//! Copyright (C) 2025 Antonio Ricciardi <dev.roothunter@gmail.com>
//!
//! This program is free software: you can redistribute it and/or modify
//! it under the terms of the GNU General Public License as published by
//! the Free Software Foundation, either version 3 of the License, or
//! (at your option) any later version.
//!
//! You should have received a copy of the GNU General Public License
//! along with this program. If not, see <https://www.gnu.org/licenses/>.

use drm_fourcc::DrmFourcc;
use evdi::buffer::BufferId;
use evdi::device_node::DeviceNode;
use evdi::{device_config::DeviceConfig, handle::Handle};
use gstreamer::glib::subclass::types::FromObject;
use gstreamer::glib::translate::{FromGlibPtrBorrow, FromGlibPtrFull, ToGlibPtr};
use gstreamer::Caps;
use gstreamer::{
    glib::object::ObjectExt,
    prelude::{ElementExt, ElementExtManual, GstBinExt},
    Buffer, FlowReturn,
};

use gstreamer_app::AppSrc;

use std::fs::OpenOptions;
use std::io::Write;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::{env, thread};
use tokio::sync::mpsc;
use tokio::task;

const AWAIT_MODE_TIMEOUT: Duration = Duration::from_secs(5);
const UPDATE_BUFFER_TIMEOUT: Duration = Duration::from_millis(33);

fn create_device() -> std::io::Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .open("/sys/devices/evdi/add")?;

    file.write_all(b"1")?; // Scrive "1" nel file per creare il dispositivo
    println!("✅ Dispositivo EVDI creato con successo");
    Ok(())
}

pub struct Frame {
    pub id: usize,
    pub buffer: Vec<u8>,
    pub width: usize,
    pub height: usize,
    pub stride: usize,
    pub pixel_format: DrmFourcc,
}

#[tokio::main] // Rende main asincrono
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env::set_var("GST_DEBUG", "3");
    gstreamer::init()?;
    //env::set_var("DISPLAY", ":0");
    let device = DeviceNode::get().unwrap();
    let device_config = DeviceConfig::sample();

    let (tx, mut rx) = mpsc::channel::<Frame>(10);

    let consumer = task::spawn(async move {
        let pipeline = gstreamer::Pipeline::new();
        let appsrc = gstreamer::ElementFactory::make("appsrc")
            .build()
            .unwrap();

        let caps_str = "video/x-raw,format=BGRx,width=1920,height=1080,framerate=60/1";

        let caps = Caps::from_str(caps_str).unwrap();
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
    });

    unsafe {

        let unconnected_handle = device.open()?;
        let mut handle = unconnected_handle.connect(&device_config);

        let mode = handle.events.await_mode(AWAIT_MODE_TIMEOUT).await?;
        let buffer_id = handle.new_buffer(&mode);

        println!("Dispositivo EVDI aperto con FD: {:?}", device);

        let mut frame_count = 0;
        let mut count = 0;

        loop {
            match handle
                .request_update(buffer_id, UPDATE_BUFFER_TIMEOUT)
                .await
            {
                Ok(_) => {

                    let buf = handle.get_buffer(buffer_id).expect("Buffer esistente");
                    println!("{:?}", buf.pixel_format);
                    let buf_data = buf.bytes(); // supponiamo che buf.bytes() restituisca una slice di byte
                    let frame_data = buf_data.to_vec(); // Clona i dati

                    tx.send(Frame { 
                        id: frame_count,
                        buffer: frame_data,
                        width: buf.width,
                        height: buf.height,
                        stride: buf.stride,
                        pixel_format: buf.pixel_format.unwrap()
                    }).await.unwrap();
                    frame_count += 1;
                }
                Err(e) => {
                    // Gestisci errore senza uscire
                    //println!("❌ Errore nell'aggiornamento del buffer: {:?}", e);
                }
            }
            count += 1;
        }
    }
    consumer.await?;

    Ok(())
}
