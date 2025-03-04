use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gstreamer::glib::clone::Downgrade;
use gstreamer::glib::object::Cast;
use gstreamer::prelude::{GObjectExtManualGst, GstBinExtManual};
use gstreamer_app::{AppSink, AppSinkCallbacks, AppSrc};
use gstreamer_rtsp_server::prelude::{
    RTSPMediaFactoryExt, RTSPMountPointsExt, RTSPServerExt, RTSPServerExtManual,
};
use std::str::FromStr;
use std::{env, fs};
use tokio::sync::mpsc::{self, Receiver, Sender};
use tokio::task::{self, JoinHandle};
use webrtc::rtp::packet::Packet;
use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;
use webrtc::track::track_local::TrackLocalWriter;

use rlink_core::protocol::frame::Frame;

use evdi::device_config::DeviceConfig;
use evdi::device_node::DeviceNode;
use gstreamer::{
    glib::object::ObjectExt,
    prelude::{ElementExt, ElementExtManual, GstBinExt},
    FlowReturn,
};
use gstreamer::{Caps, ElementFactory, FlowSuccess, Format, Pipeline};
use gstreamer_rtsp_server::{RTSPMediaFactory, RTSPMountPoints, RTSPServer, RTSPTransportMode};

use rlink_core::protocol::rtp_header::RtpHeader;

pub const AWAIT_MODE_TIMEOUT: Duration = Duration::from_secs(5);
pub const UPDATE_BUFFER_TIMEOUT: Duration = Duration::from_millis(33);

#[derive(Debug, Clone)]
pub struct ScreenConfig {
    width: usize,
    height: usize,
    framerate: usize,
    format: gstreamer_video::VideoFormat,
    timeout_await_mode: Duration,
    timeout_update_buffer: Duration,
}

pub struct Screen {
    config: ScreenConfig,
    channel_tx: Option<Arc<Mutex<Sender<Frame>>>>,
    pub pipeline: Option<Pipeline>,
    pub appsink: Option<AppSink>,
    pub appsrc_thread: Option<JoinHandle<()>>,
}

impl Screen {
    pub fn new(width: usize, height: usize, framerate: usize) -> Self {
        let format = gstreamer_video::VideoFormat::Bgrx;

        return Screen {
            config: ScreenConfig {
                format,
                width,
                height,
                framerate,
                timeout_await_mode: AWAIT_MODE_TIMEOUT,
                timeout_update_buffer: UPDATE_BUFFER_TIMEOUT,
            },
            pipeline: None,
            appsink: None,
            appsrc_thread: None,
            channel_tx: None,
        };
    }

    pub fn init(&mut self, video_track: Option<Arc<TrackLocalStaticRTP>>) {
        let config = self.config.clone();

        //self.setup_xvimagesink(config);
        //self.setup_rtsp_server(config);    }
        //self.setup_rtp_sink(config, "0.0.0.0", 6000);

        //return None;
        self.setup_udpsink(config, video_track);
    }

    pub async fn start(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let device = DeviceNode::get().unwrap();
        let device_config = DeviceConfig::sample();

        let channel_tx = self.channel_tx.clone().unwrap();
        let tx = channel_tx.lock().unwrap();

        unsafe {
            let unconnected_handle = device.open()?;
            let mut handle = unconnected_handle.connect(&device_config);

            let mode = handle.events.await_mode(AWAIT_MODE_TIMEOUT).await?;
            let buffer_id = handle.new_buffer(&mode);

            println!("Dispositivo EVDI aperto con FD: {:?}", device);

            let mut frame_count = 0;

            loop {
                match handle
                    .request_update(buffer_id, UPDATE_BUFFER_TIMEOUT)
                    .await
                {
                    Ok(_) => {
                        let buf = handle.get_buffer(buffer_id).expect("Buffer esistente");
                        let buf_data = buf.bytes();
                        let frame_data = buf_data.to_vec();

                        tx.send(Frame {
                            id: frame_count,
                            buffer: frame_data,
                            width: buf.width,
                            height: buf.height,
                            stride: buf.stride,
                            pixel_format: buf.pixel_format.unwrap(),
                        })
                        .await
                        .unwrap();
                        frame_count += 1;
                    }
                    Err(_) => {
                        //println!("Error: {:?}", e);
                    }
                }
            }
        }
    }

    fn get_src_caps(config: ScreenConfig) -> gstreamer::Caps {
        let caps_str = format!(
            "video/x-raw,format={},width={},height={},framerate={}/1",
            config.format, config.width, config.height, config.framerate
        );
        let caps_str = caps_str.as_str();
        return gstreamer::Caps::from_str(caps_str).unwrap();
    }

    fn setup<F>(&mut self, setup_pipeline: F)
    where
        F: Fn() -> (),
    {
        // let (tx, mut rx) = mpsc::channel::<Frame>(10);
        // let mut frame_count = 0;
        // setup_pipeline();
        // self.appsrc_thread = Some(task::spawn(async move {
        //     while let Some(frame) = rx.recv().await {
        //         //println!("📡 Ricevuto frame di {:?} bytes", buffer.len());
        //         let gsbuffer = gstreamer::buffer::Buffer::from_slice(frame.buffer);
        //         match appsrc.push_buffer(gsbuffer) {
        //             Ok(FlowSuccess::Ok) => (),
        //             Ok(_) => eprintln!("Frame push returned non-Ok flow"),
        //             Err(err) => eprintln!("Error pushing frame: {:?}", err),
        //         }
        //     }
        // }));
        //self.channel_tx = Some(Arc::new(Mutex::new(tx)));
    }

    fn setup_x11_sink(&mut self, config: ScreenConfig) {
        self.setup(|| {
            let caps = Screen::get_src_caps(config.clone());

            let pipeline = Pipeline::new();
            let src = ElementFactory::make("appsrc")
                .property("caps", caps)
                .property("is-live", true)
                .build()
                .unwrap();

            let sink = ElementFactory::make("xvimagesink").build().unwrap();

            pipeline.add_many(&[&src, &sink]).unwrap();

            src.link(&sink).unwrap();
            pipeline.set_state(gstreamer::State::Playing).unwrap();
        });
    }

    fn setup_rtp_sink(&mut self, config: ScreenConfig, host: &str, port: u16) {
        let (tx, mut rx) = mpsc::channel::<Frame>(10);
        self.channel_tx = Some(Arc::new(Mutex::new(tx)));

        let caps = Screen::get_src_caps(config.clone());

        let pipeline = Pipeline::new();
        let appsrc = ElementFactory::make("appsrc")
            .property("caps", caps)
            .property("is-live", true)
            .property("format", gstreamer::Format::Time)
            .build()
            .unwrap();
        let queue = ElementFactory::make("queue").build().unwrap();
        let convert = ElementFactory::make("videoconvert").build().unwrap();
        // Encoder H.264
        let encoder = ElementFactory::make("x264enc")
            .property_from_str("bitrate", "8000") // Bitrate in kbps
            .property("byte-stream", true)
            .property_from_str("speed-preset", "ultrafast")
            .property_from_str("tune", "zerolatency")
            .build()
            .unwrap();

        // Packetizzazione RTP per H.264
        let rtp_pay = ElementFactory::make("rtph264pay")
            //.property("config-interval", 1) // Necessario per alcuni player RTP
            .property_from_str("aggregate-mode", "zero-latency")
            .build()
            .unwrap();

        // UDP Sink per inviare il flusso RTP
        let appsink = ElementFactory::make("udpsink")
            .property("host", host)
            .property("port", port as i32)
            .property("sync", true) // Se vuoi sincronizzazione con il clock di sistema
            .property("async", true)
            .build()
            .unwrap();

        pipeline
            .add_many(&[&appsrc, &queue, &convert, &encoder, &rtp_pay, &appsink])
            .unwrap();
        appsrc.link(&queue).unwrap();
        queue.link(&convert).unwrap();
        convert.link(&encoder).unwrap();
        encoder.link(&rtp_pay).unwrap();
        rtp_pay.link(&appsink).unwrap();

        pipeline.set_state(gstreamer::State::Playing).unwrap();

        let appsrc = appsrc.downcast::<AppSrc>().unwrap();

        self.appsrc_thread = Some(task::spawn(async move {
            while let Some(frame) = rx.recv().await {
                //println!("📡 Ricevuto frame di {:?} bytes", buffer.len());
                let gsbuffer = gstreamer::buffer::Buffer::from_slice(frame.buffer);
                match appsrc.push_buffer(gsbuffer) {
                    Ok(FlowSuccess::Ok) => (),
                    Ok(_) => eprintln!("Frame push returned non-Ok flow"),
                    Err(err) => eprintln!("Error pushing frame: {:?}", err),
                }
            }
        }));
    }

    fn setup_udpsink(
        &mut self,
        config: ScreenConfig,
        video_track: Option<Arc<TrackLocalStaticRTP>>,
    ) {
        let (tx, mut rx) = mpsc::channel::<Frame>(10);
        self.channel_tx = Some(Arc::new(Mutex::new(tx)));

        let caps = Screen::get_src_caps(config.clone());

        let pipeline = Pipeline::new();
        let appsrc = ElementFactory::make("appsrc")
            .property("caps", caps)
            .property("is-live", true)
            .property("format", gstreamer::Format::Time)
            .build()
            .unwrap();

        let queue = ElementFactory::make("queue").build().unwrap();
        let convert = ElementFactory::make("videoconvert").build().unwrap();
        // Encoder H.264
        let encoder = ElementFactory::make("x264enc")
            .property_from_str("bitrate", "500") // Bitrate in kbps
            .property("byte-stream", true)
            .property_from_str("speed-preset", "ultrafast")
            .property_from_str("tune", "zerolatency")
            .build()
            .unwrap();

        // Packetizzazione RTP per H.264
        let rtp_pay = ElementFactory::make("rtph264pay")
            .property("config-interval", 1) // Necessario per alcuni player RTP
            //.property_from_str("aggregate-mode", "zero-latency")
            .property_from_str("pt", "96") // Necessario per alcuni player RTP
            .build()
            .unwrap();

        let udpsink = ElementFactory::make("udpsink")
            .property("host", "0.0.0.0") // Necessario per alcuni player RTP
            .property("port", 9544) // Necessario per alcuni player RTP
            .build()
            .unwrap();

        pipeline
            .add_many(&[&appsrc, &queue, &convert, &encoder, &rtp_pay, &udpsink])
            .unwrap();

        appsrc.link(&queue).unwrap();
        queue.link(&convert).unwrap();
        convert.link(&encoder).unwrap();
        encoder.link(&rtp_pay).unwrap();
        rtp_pay.link(&udpsink).unwrap();

        pipeline.set_state(gstreamer::State::Playing).unwrap();

        let appsrc = appsrc.downcast::<AppSrc>().unwrap();
        self.appsrc_thread = Some(task::spawn(async move {
            while let Some(frame) = rx.recv().await {
                //println!("📡 Ricevuto frame di {:?} bytes", buffer.len());
                let gsbuffer = gstreamer::buffer::Buffer::from_slice(frame.buffer);
                match appsrc.push_buffer(gsbuffer) {
                    Ok(FlowSuccess::Ok) => (),
                    Ok(_) => eprintln!("Frame push returned non-Ok flow"),
                    Err(err) => eprintln!("Error pushing frame: {:?}", err),
                }
            }
        }));
    }
}