use evdi::{device_config::DeviceConfig, handle::Handle};
use evdi::device_node::DeviceNode;
use gstreamer::{
    glib::object::ObjectExt,
    prelude::{ElementExt, ElementExtManual, GstBinExt},
    Buffer, FlowReturn,
};
use std::env;
use std::fs::OpenOptions;
use std::io::Write;
use std::time::Duration;
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
    //create_device()?; //REQUIRE KERNEL SPACE PERMISSION
    unsafe {
        //env::set_var("DISPLAY", ":0");
        env::set_var("GST_DEBUG", "3");

        gstreamer::init()?;
        let pipeline = gstreamer::Pipeline::new();
        //let appsrc = gstreamer::ElementFactory::make("videotestsrc").build().unwrap();
        let appsrc = gstreamer::ElementFactory::make("videotestsrc")
            .build()
            .unwrap();
        pipeline.add(&appsrc).unwrap();

        let sink = gstreamer::ElementFactory::make("glimagesink")
            .build()
            .unwrap();
        pipeline.add(&sink).unwrap();

        appsrc.link(&sink).unwrap();
        pipeline.set_state(gstreamer::State::Playing).unwrap();

        let device = DeviceNode::get().unwrap();
        let device_config = DeviceConfig::sample();

        sleep(Duration::from_secs(1)).await;

        let unconnected_handle = device.open()?;
        let mut handle = unconnected_handle.connect(&device_config);
        println!("Dispositivo EVDI aperto con FD: {:?}", device);

        let mode = handle.events.await_mode(AWAIT_MODE_TIMEOUT).await?;
        let buffer_id = handle.new_buffer(&mode);

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
