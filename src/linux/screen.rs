use std::sync::{Arc, Mutex};
use std::time::Duration;

use std::env;
use std::str::FromStr;

use tokio::sync::mpsc::{self, Sender};
use tokio::task::{self, JoinHandle};

use rlink_core::protocol::frame::Frame;

use evdi::device_config::DeviceConfig;
use evdi::device_node::DeviceNode;
use gstreamer::Caps;
use gstreamer::{
    glib::object::ObjectExt,
    prelude::{ElementExt, ElementExtManual, GstBinExt},
    FlowReturn,
};

pub const AWAIT_MODE_TIMEOUT: Duration = Duration::from_secs(5);
pub const UPDATE_BUFFER_TIMEOUT: Duration = Duration::from_millis(33);

#[derive(
    Debug, Clone
)]
pub struct ScreenConfig {
    width: usize,
    height: usize,
    framerate: usize,
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
        return Screen {
            config: ScreenConfig{
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
        env::set_var("GST_DEBUG", "3");

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

    fn get_caps_str(config: ScreenConfig) -> String{
        return format!(
            "video/x-raw,format=BGRx,width={},height={},framerate={}/1", 
            config.width, config.height, config.framerate
        );
    }

    fn init_channel(&mut self) {
        let config = self.config.clone();
        let (tx, mut rx) = mpsc::channel::<Frame>(10);

        self.handler = Some(task::spawn(async move {
            let pipeline = gstreamer::Pipeline::new();
            let appsrc = gstreamer::ElementFactory::make("appsrc").build().unwrap();

            let caps_str = Screen::get_caps_str(config);
            let caps = Caps::from_str(caps_str.as_str()).unwrap();
            appsrc.set_property("caps", caps);

            pipeline.add(&appsrc).unwrap();
            let sink = gstreamer::ElementFactory::make("glimagesink")
                .build()
                .unwrap();
            pipeline.add(&sink).unwrap();

            appsrc.link(&sink).unwrap();

            pipeline.set_state(gstreamer::State::Playing).unwrap();

            while let Some(frame) = rx.recv().await {
                let buffer = frame.buffer;

                println!("📡 Ricevuto frame di {:?} bytes", buffer.len());
                let gsbuffer = gstreamer::buffer::Buffer::from_slice(buffer);

                let result: FlowReturn = appsrc.emit_by_name("push-buffer", &[&gsbuffer]);
                if result != gstreamer::FlowReturn::Ok {
                    eprintln!("❌ Errore nel push del buffer: {:?}", result);
                }
            }
        }));
        
        self.channel_tx = Some(Arc::new(Mutex::new(tx)));
    }
}
