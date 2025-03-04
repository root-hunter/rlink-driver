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
use gstreamer::FlowSuccess;
use gstreamer_app::AppSinkCallbacks;
use linux::screen::*;
use std::env;
use std::sync::Arc;
use tokio::sync::Mutex;
use warp::cors::cors;
use warp::{Filter, Reply};
use webrtc::{
    api::{media_engine::MediaEngine, APIBuilder},
    ice_transport::ice_candidate::RTCIceCandidate,
    peer_connection::{
        configuration::RTCConfiguration, sdp::session_description::RTCSessionDescription,
        RTCPeerConnection,
    },
    rtp::packet::Packet,
    rtp_transceiver::rtp_codec::{RTCRtpCodecCapability, RTCRtpCodecParameters, RTPCodecType},
    track::track_local::{
        track_local_static_rtp::TrackLocalStaticRTP, TrackLocal, TrackLocalWriter,
    },
};


#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env::set_var("GST_DEBUG", "3");
    gstreamer::init()?;

    let mut screen = Screen::new(1920, 1080, 30);
    screen.init(None);
    screen.start().await?;

    Ok(())
}
