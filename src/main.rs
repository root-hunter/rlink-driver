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
    let appsink = screen.init();
    screen.start().await?;

    // let mut m = MediaEngine::default();
    // m.register_codec(
    //     RTCRtpCodecParameters {
    //         stats_id: "".into(),
    //         payload_type: 96,
    //         capability: RTCRtpCodecCapability {
    //             mime_type: "video/x-264".into(),
    //             clock_rate: 90000,
    //             channels: 0,
    //             sdp_fmtp_line:
    //                 "level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42001f"
    //                     .to_string(),
    //             rtcp_feedback: vec![],
    //         },
    //     },
    //     RTPCodecType::Video,
    // )?;

    // let api = APIBuilder::new().with_media_engine(m).build();
    // let config = RTCConfiguration::default();
    // let peer_connection = Arc::new(Mutex::new(api.new_peer_connection(config).await?));

    // let video_track = Arc::new(TrackLocalStaticRTP::new(
    //     RTCRtpCodecCapability {
    //         mime_type: "video/x-264".into(),
    //         clock_rate: 90000,
    //         channels: 0,
    //         sdp_fmtp_line: "level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42001f"
    //             .to_string(),
    //         rtcp_feedback: vec![],
    //     },
    //     "video".to_string(),
    //     "webrtc-rs-video".to_string(),
    // ));

    // {
    //     let mut pc = peer_connection.lock().await;
    //     pc.add_track(Arc::clone(&video_track) as Arc<dyn TrackLocal + Send + Sync>)
    //         .await?;

    //     println!("STO BLOCCANDO ICE");
    //     pc.on_ice_candidate(Box::new(move |candidate: Option<RTCIceCandidate>| {
    //         if let Some(candidate) = candidate {
    //             println!("Generated ICE Candidate: {:?}", candidate.to_string());
    //         } else {
    //             println!("ICE Gathering complete.");
    //         }
    //         Box::pin(async {})
    //     }));
    // }

    // let video_track_clone = video_track.clone(); // Clona la traccia video

    // appsink.set_callbacks(
    //     AppSinkCallbacks::builder()
    //         .new_sample(move |appsink| {
    //             let sample = appsink.pull_sample().unwrap();
    //             let buffer = sample.buffer().unwrap();
    //             let map = buffer.map_readable().unwrap();
    //             let data = map.as_slice();

    //             let _ = video_track_clone.write_rtp(&Packet {
    //                 header: webrtc::rtp::header::Header {
    //                     ..Default::default()
    //                 },
    //                 payload: data.to_vec().into(),
    //             });

    //             Ok(FlowSuccess::Ok)
    //         })
    //         .build(),
    // );

    // let pc_clone2 = peer_connection.clone();
    // let offer_route = warp::post()
    //     .and(warp::path("offer"))
    //     .and(warp::body::json())
    //     .and_then(move |offer: RTCSessionDescription| {
    //         let pc = pc_clone2.clone();
    //         async move {
    //             let mut locked_pc = pc.lock().await;
    //             println!("Offer received: {:?}", offer);

    //             if locked_pc.signaling_state()
    //                 == webrtc::peer_connection::signaling_state::RTCSignalingState::HaveRemoteOffer
    //             {
    //                 eprintln!("Duplicate offer received, ignoring.");
    //                 return Ok::<_, warp::Rejection>(warp::reply::with_status(
    //                     warp::reply::json(&"Duplicate offer"),
    //                     warp::http::StatusCode::BAD_REQUEST,
    //                 ));
    //             }

    //             match locked_pc.set_remote_description(offer).await {
    //                 Ok(_) => match locked_pc.create_answer(None).await {
    //                     Ok(answer) => {
    //                         println!("Answer sent: {:?}", answer);
    //                         match locked_pc.set_local_description(answer.clone()).await {
    //                             Ok(_) => Ok::<_, warp::Rejection>(warp::reply::with_status(
    //                                 warp::reply::json(&answer),
    //                                 warp::http::StatusCode::ACCEPTED,
    //                             )),
    //                             Err(e) => {
    //                                 eprintln!("Error setting local description: {:?}", e);
    //                                 Err(warp::reject::reject())
    //                             }
    //                         }
    //                     }
    //                     Err(e) => {
    //                         eprintln!("Error creating answer: {:?}", e);
    //                         Err(warp::reject::reject())
    //                     }
    //                 },
    //                 Err(e) => {
    //                     eprintln!("Error setting remote description: {:?}", e);
    //                     Err(warp::reject::reject())
    //                 }
    //             }
    //         }
    //     });

    // tokio::spawn(async move {
    //     let cors = cors()
    //         .allow_any_origin()
    //         .allow_headers(vec!["Content-Type"])
    //         .allow_methods(vec!["GET", "POST"]);

    //     let routes = offer_route.with(cors); // Aggiungi CORS alla route

    //     warp::serve(routes).run(([127, 0, 0, 1], 3030)).await;
    // });


    Ok(())
}
