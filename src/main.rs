//! Copyright (C) 2025 Antonio Ricciardi <dev.roothunter@gmail.com>
//!
//! This program is free software: you can redistribute it and/or modify
//! it under the terms of the GNU General Public License as published by
//! the Free Software Foundation, either version 3 of the License, or
//! (at your option) any later version.
//!
//! You should have received a copy of the GNU General Public License
//! along with this program. If not, see <https://www.gnu.org/licenses/>.

use evdi::device_node::DeviceNode;
use evdi::{device_config::DeviceConfig, handle::Handle};
use gstreamer::glib::subclass::types::FromObject;
use gstreamer::glib::translate::{FromGlibPtrBorrow, FromGlibPtrFull, ToGlibPtr};
use gstreamer::{
    glib::object::ObjectExt,
    prelude::{ElementExt, ElementExtManual, GstBinExt},
    Buffer, FlowReturn,
};

use gstreamer_app::AppSrc;

use std::fs::OpenOptions;
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::{env, thread};
use tokio::time::sleep;

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

#[tokio::main] // Rende main asincrono
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env::set_var("GST_DEBUG", "3");
    gstreamer::init()?;

    let device = DeviceNode::get().unwrap();
    let device_config = DeviceConfig::sample();

    unsafe {
        //env::set_var("DISPLAY", ":0");

        let unconnected_handle = device.open()?;
        let mut handle = unconnected_handle.connect(&device_config);

        let mode = handle.events.await_mode(AWAIT_MODE_TIMEOUT).await?;
        let buffer_id = handle.new_buffer(&mode);

        let pipeline = gstreamer::Pipeline::new();
        //let appsrc = gstreamer::ElementFactory::make("videotestsrc").build().unwrap();
        
        
        let appsrc = gstreamer::ElementFactory::make("videotestsrc")
            .build()
            .unwrap();
        let appsrc = AppSrc::from_(appsrc);

        pipeline.add(&appsrc).unwrap();

        let sink = gstreamer::ElementFactory::make("glimagesink")
            .build()
            .unwrap();
        pipeline.add(&sink).unwrap();

        appsrc.link(&sink).unwrap();
        pipeline.set_state(gstreamer::State::Playing).unwrap();

        println!("Dispositivo EVDI aperto con FD: {:?}", device);

        sleep(Duration::from_millis(100)).await;

        let mut frames = 0;
        let mut count = 0;

        loop {
            match handle
                .request_update(buffer_id, UPDATE_BUFFER_TIMEOUT)
                .await
            {
                Ok(_) => {
                    let buf = handle.get_buffer(buffer_id).expect("Buffer esistente");
                    let gsbuffer = Buffer::from_slice(buf.bytes());

                    appsrc.pus

                    // Invia il buffer tramite appsrc
                    let result: FlowReturn = appsrc.emit_by_name("push-buffer", &[&gsbuffer]);
                    if result != gstreamer::FlowReturn::Ok {
                        eprintln!("❌ Errore nel push del buffer: {:?}", result);
                    }
                    frames += 1;
                }
                Err(e) => {
                    // Gestisci errore senza uscire
                    //println!("❌ Errore nell'aggiornamento del buffer: {:?}", e);
                }
            }
            println!("Count: {} Frames: {}", count, frames);
            count += 1;
        }
    }

    Ok(())
}
