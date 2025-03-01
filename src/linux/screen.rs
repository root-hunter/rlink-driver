use std::sync::{Arc, Mutex};
use std::time::Duration;

use std::env;
use std::str::FromStr;

use gstreamer::prelude::{GObjectExtManualGst, GstBinExtManual};
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
use gstreamer::{Caps, ElementFactory, Pipeline};

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
    handler: Option<JoinHandle<()>>,
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
            handler: None,
            channel_tx: None,
        };
    }

    pub fn init(&mut self) {
        self.init_channel();
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

    fn get_appsrc_caps(config: ScreenConfig) -> gstreamer::Caps {
        let caps_str = format!(
            "video/x-raw,format={},width={},height={},framerate={}/1",
            config.format, config.width, config.height, config.framerate
        );
        let caps_str = caps_str.as_str();
        return gstreamer::Caps::from_str(caps_str).unwrap();
    }

    fn setup<F>(&mut self, setup_pipeline: F)
    where
        F: Fn() -> gstreamer::Element,
    {
        let (tx, mut rx) = mpsc::channel::<Frame>(10);
        let mut frame_count = 0;
        let appsrc = setup_pipeline();

        self.handler = Some(task::spawn(async move {
            while let Some(frame) = rx.recv().await {
                //println!("📡 Ricevuto frame di {:?} bytes", buffer.len());
                let gsbuffer = gstreamer::buffer::Buffer::from_slice(frame.buffer);

                let result: FlowReturn = appsrc.emit_by_name("push-buffer", &[&gsbuffer]);
                if result != gstreamer::FlowReturn::Ok {
                    eprintln!("❌ Errore nel push del buffer: {:?}", result);
                } else {
                    frame_count += 1;
                }
            }
        }));
        self.channel_tx = Some(Arc::new(Mutex::new(tx)));
    }

    fn setup_xvimagesink(&mut self, config: ScreenConfig) {
        self.setup(|| {
            let caps = Screen::get_appsrc_caps(config.clone());

            let pipeline = Pipeline::new();
            let appsrc = ElementFactory::make("appsrc")
                .property("caps", caps)
                .property("is-live", true)
                .build()
                .unwrap();

            let sink = ElementFactory::make("xvimagesink").build().unwrap();

            pipeline.add_many(&[&appsrc, &sink]).unwrap();

            appsrc.link(&sink).unwrap();
            pipeline.set_state(gstreamer::State::Playing).unwrap();

            return appsrc;
        });
    }

    fn setup_xvimagesink_compressed(&mut self, config: ScreenConfig) {
        self.setup(|| {
            let pipeline = Pipeline::new();

            let appsrc = ElementFactory::make("appsrc").build().unwrap();
            let queue = ElementFactory::make("queue").build().unwrap();
            let x264enc = ElementFactory::make("x264enc").build().unwrap();
            let h264parse = ElementFactory::make("h264parse").build().unwrap();
            let avdec_h264 = ElementFactory::make("avdec_h264").build().unwrap();
            let videoconvert1 = ElementFactory::make("videoconvert").build().unwrap();
            let videoconvert2 = ElementFactory::make("videoconvert").build().unwrap();
            let autovideosink = ElementFactory::make("autovideosink").build().unwrap();

            let appsrc_caps =
                Caps::from_str(&"video/x-raw,format=BGRx,width=1920,height=1080,framerate=60/1")
                    .unwrap();

            appsrc.set_property("caps", appsrc_caps);
            appsrc.set_property("is-live", true);
            appsrc.set_property("do-timestamp", true);

            // Aggiungi gli elementi alla pipeline
            pipeline
                .add_many(&[
                    &appsrc,
                    &queue,
                    &videoconvert1,
                    &x264enc,
                    &h264parse,
                    &avdec_h264,
                    &videoconvert2,
                    &autovideosink,
                ])
                .unwrap();

            // Collega gli elementi
            appsrc.link(&queue).unwrap();
            queue.link(&videoconvert1).unwrap();
            videoconvert1.link(&x264enc).unwrap();
            x264enc.link(&h264parse).unwrap();
            h264parse.link(&avdec_h264).unwrap();
            avdec_h264.link(&videoconvert2).unwrap();
            videoconvert2.link(&autovideosink).unwrap();

            // Imposta lo stato della pipeline su Playing
            pipeline.set_state(gstreamer::State::Playing).unwrap();

            return appsrc;
        });
    }

    fn setup_udpsink(&mut self, config: ScreenConfig) {
        self.setup(|| {
            let caps_str = "format=BGRx";
            let caps = Caps::from_str(caps_str).unwrap();

            let pipeline = Pipeline::new();
            let appsrc = ElementFactory::make("appsrc").build().unwrap();
            let queue = ElementFactory::make("queue").build().unwrap();
            let videoconvert = ElementFactory::make("videoconvert").build().unwrap();
            let encoder = gstreamer::ElementFactory::make("x264enc").build().unwrap();
            let queue2 = ElementFactory::make("queue").build().unwrap();
            let sink = ElementFactory::make("autovideosink").build().unwrap();

            appsrc.set_property("caps", caps);
            //appsrc.set_property("is-live", true);
            //appsrc.set_property("do-timestamp", true);

            pipeline
                .add_many(&[
                    &appsrc,
                    &queue,
                    &videoconvert, // Aggiungi l'encoder
                    &encoder,      // Aggiungi l'encoder
                    &queue2,       // Aggiungi l'encoder
                    &sink,
                ])
                .unwrap();

            appsrc.link(&queue).unwrap();
            queue.link(&videoconvert).unwrap(); // Collega a encoder
            videoconvert.link(&encoder).unwrap(); // Collega a encoder
            encoder.link(&queue2).unwrap(); // Collega il sink all'encoder
            queue2.link(&sink).unwrap(); // Collega il sink all'encoder

            pipeline.set_state(gstreamer::State::Playing).unwrap();

            return appsrc;
        });
    }

    fn init_channel(&mut self) {
        let config = self.config.clone();

        //self.setup_compressed_xvimagesink(config);
        self.setup_xvimagesink(config);
    }
}
