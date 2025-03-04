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
use webrtc::rtp::packet::Packet;
use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;
use webrtc::track::track_local::TrackLocalWriter;
use std::str::FromStr;
use std::{env, fs};
use tokio::sync::mpsc::{self, Receiver, Sender};
use tokio::task::{self, JoinHandle};

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

    pub fn init(&mut self, video_track: Option<Arc<TrackLocalStaticRTP>>) -> Option<AppSink> {
        let config = self.config.clone();

        //self.setup_xvimagesink(config);
        //self.setup_rtsp_server(config);    }
        //self.setup_rtp_sink(config, "0.0.0.0", 6000);

        //return None;
        self.setup_appsink(config, video_track);
        return Some(self.appsink.as_ref().unwrap().clone());
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

    fn setup_appsink(&mut self, config: ScreenConfig, video_track: Option<Arc<TrackLocalStaticRTP>>) {
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

        let appsink = ElementFactory::make("appsink").build().unwrap();
        let appsink_cast = appsink.clone().downcast::<AppSink>().unwrap();

        let video_track_clone = video_track.clone();

        appsink_cast.set_callbacks(
            AppSinkCallbacks::builder()
                .new_sample(move |appsink| {
                    let sample = appsink.pull_sample().unwrap();
                    let buffer = sample.buffer().unwrap();
                    let map = buffer.map_readable().unwrap();
                    let data = map.as_slice();
    
                    let rtp_header = RtpHeader::new(&data);

                    println!("HEADER: {:#?}", rtp_header);

                    // let _ = video_track_clone.write_rtp(&Packet {
                    //     header: webrtc::rtp::header::Header {
                    //         ..Default::default()
                    //     },
                    //     payload: data.to_vec().into(),
                    // });
    
                    Ok(FlowSuccess::Ok)
                })
                .build(),
        );

        pipeline
            .add_many(&[&appsrc, &queue, &convert, &encoder, &rtp_pay, &appsink])
            .unwrap();
        self.appsink = appsink_cast.into();

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
}

// let appsink = sink.clone().downcast::<AppSink>().unwrap(); // Conversione a AppSink

// appsink.set_property("emit-signals", true);
// let appsink_clone = appsink.clone();
// let frame_count = std::sync::Mutex::new(0); // Contatore di frame
// let callbacks = AppSinkCallbacks::builder()
//     .new_sample(move |appsink| {
//         let sample = appsink.pull_sample().unwrap();
//         let buffer = sample.buffer().unwrap();
//         let map = buffer.map_readable().unwrap();
//         let data = map.as_slice();
//         println!("Ricevuto buffer H.264 di dimensione: {}", data.len());
//         // Genera un nome di file univoco
//         let mut count = frame_count.lock().unwrap();
//         let filename = format!(
//             "/mnt/07278d6f-dcd5-4540-ae3f-dc7f08c050e4/Dev/rlink/samples/frame_{}.h264",
//             *count
//         );
//         *count += 1;
//         drop(count); // Rilascia il lock

//         // Scrivi i dati del buffer nel file
//         let path = Path::new(&filename);
//         if let Err(e) = fs::write(path, data) {
//             eprintln!("Errore durante la scrittura del file {}: {}", filename, e);
//         }

//         Ok(FlowSuccess::Ok)
//     })
//     .build();

// appsink_clone.set_callbacks(callbacks);
//fn setup_rtsp_server(&mut self, config: ScreenConfig) {
//     self.setup(|| {
//         // Configura le capacità del flusso video
//         let caps = Screen::get_src_caps(config.clone());

//         // Crea il pipeline
//         let pipeline = Pipeline::new();

//         // Appsrc per acquisire il flusso video
//         let appsrc = ElementFactory::make("appsrc")
//             .property("caps", caps)
//             .property("is-live", true)
//             .property("format", Format::Time)
//             .build()
//             .unwrap();

//         let queue = ElementFactory::make("queue").build().unwrap();
//         let convert = ElementFactory::make("videoconvert").build().unwrap();

//         // Encoder H.264
//         let encoder = ElementFactory::make("x264enc")
//             .property_from_str("bitrate", "8000") // Bitrate in kbps
//             .property("byte-stream", true)
//             .property_from_str("speed-preset", "ultrafast")
//             .property_from_str("tune", "zerolatency")
//             .build()
//             .unwrap();

//         // RTP payload per H.264
//         let rtp_pay = ElementFactory::make("rtph264pay")
//             .property_from_str("pt", "96") // Payload type per H.264
//             .build()
//             .unwrap();

//         pipeline.add_many(&[&appsrc, &queue, &convert, &encoder, &rtp_pay]).unwrap();

//         appsrc.link(&queue).unwrap();
//         queue.link(&convert).unwrap();
//         convert.link(&encoder).unwrap();
//         encoder.link(&rtp_pay).unwrap();

//         // Configura il factory per il server RTSP
//         let mount_point = "/test";
//         let port = 9999;
//         let server = RTSPServer::new();
//         let factory = RTSPMediaFactory::new();
//         let mounts = RTSPMountPoints::new();
//         factory.set_launch(&format!(
//             "( appsrc name=source ! queue ! videoconvert ! x264enc bitrate=8000 speed-preset=ultrafast tune=zerolatency ! rtph264pay name=pay0 pt=96 )"
//         ));
//         factory.set_transport_mode(RTSPTransportMode::PLAY);
//         factory.set_shared(true);
//         mounts.add_factory(mount_point, factory);
//         server.set_mount_points(Some(&mounts));
//         server.set_service(&port.to_string());

//         server.attach(None).unwrap();

//         let media = factory.element().unwrap();
//         let pipeline = media.get_pipeline().unwrap();

//         // Recupera appsrc dalla pipeline
//         let appsrc = pipeline.get_by_name("source").unwrap();

//         // Stampa il messaggio di avvio
//         println!(
//             "Server RTSP avviato su rtsp://127.0.0.1:{}{}",
//             port, mount_point
//         );

//         // Restituisci il pipeline e l'appsrc
//         return (pipeline, appsrc, encoder);
//     });
// }
