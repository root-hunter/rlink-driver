//! Copyright (C) 2025 Antonio Ricciardi <dev.roothunter@gmail.com>
//!
//! This program is free software: you can redistribute it and/or modify
//! it under the terms of the GNU General Public License as published by
//! the Free Software Foundation, either version 3 of the License, or
//! (at your option) any later version.
//!
//! You should have received a copy of the GNU General Public License
//! along with this program. If not, see <https://www.gnu.org/licenses/>.

mod linux;
use gstreamer_rtsp_server::{
    prelude::{RTSPMediaFactoryExt, RTSPMountPointsExt, RTSPServerExt, RTSPServerExtManual},
    RTSPMediaFactory, RTSPServer, RTSPTransportMode,
};
use linux::screen::*;

use std::env;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env::set_var("GST_DEBUG", "3");
    gstreamer::init()?;

    let mut screen = Screen::new(1920, 1080, 60);
    screen.init();
    screen.start().await?;

    Ok(())
}
