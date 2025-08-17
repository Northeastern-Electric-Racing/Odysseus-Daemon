use std::{error::Error, time::UNIX_EPOCH};

use futures_util::SinkExt;
use gpsd_proto::{Pps, Tpv, UnifiedResponse};
use tokio::{net::TcpStream, sync::mpsc::Sender};
use tokio_stream::StreamExt::{self};
use tokio_util::{
    codec::{Framed, LinesCodec},
    sync::CancellationToken,
};
use tracing::{debug, info, trace, warn};

use crate::PublishableMessage;

/// changes settings and sends GPS data
pub async fn gps_manager(
    cancel_token: CancellationToken,
    mqtt_sender_tx: Sender<PublishableMessage>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    // lets create some GPS
    let stream = TcpStream::connect("127.0.0.1:2947").await?;
    let mut framed = Framed::new(stream, LinesCodec::new());
    if let Err(e) = framed.send(gpsd_proto::ENABLE_WATCH_CMD).await {
        warn!("Could not watch GPS: {e}");
    }

    loop {
        tokio::select! {
            _ = cancel_token.cancelled() => {
                debug!("Quitting GPS Handler");
                break Ok(());
            },
            res = framed.next() => {
                let time = UNIX_EPOCH.elapsed().unwrap().as_micros() as u64;
                match res {
                    Some(msg) => {
                        match msg {
                            Ok(msg) => {
                                if let Some(data) = handle_gps_msg(msg).await {
                                    send_gps_data(data, &mqtt_sender_tx, time).await;
                                }

                            },
                            Err(err) => {
                                warn!("Error decoding GPS message {err}");
                            }
                        }
                    },
                    None => {
                        warn!("Connected to GPS over, exiting!");
                        break Ok(());
                    },
                }
            }
        }
    }
}

async fn handle_gps_msg(msg: String) -> Option<UnifiedResponse> {
    trace!("Recieved GPS message {msg}");
    match serde_json::from_str(&msg) {
        Ok(a) => Some(a),
        Err(e) => {
            warn!("Could not decode GPS message: {msg} -- {e}");
            None
        }
    }
}

async fn send_gps_data(
    data: UnifiedResponse,
    mqtt_sender_tx: &Sender<PublishableMessage>,
    time: u64,
) {
    let msgs = match data {
        UnifiedResponse::Version(v) => {
            trace!("Got GPS Version: {:?}", v);
            info!(
                "Connected to GPSD {}.{}.{}",
                v.proto_major, v.proto_minor, v.release
            );
            return;
        }
        UnifiedResponse::Devices(devices) => {
            trace!("Got GPS devices: {:?}", devices);
            return;
        }
        UnifiedResponse::Watch(watch) => {
            trace!("Got GPS watch: {:?}", watch);
            return;
        }
        UnifiedResponse::Device(device) => {
            trace!("Got GPS device: {:?}", device);
            return;
        }
        UnifiedResponse::Tpv(tpv) => {
            trace!("Got GPS Tpv: {:?}", tpv);
            parse_tpv(tpv, time)
        }
        UnifiedResponse::Sky(sky) => {
            trace!("Got GPS sky: {:?}", sky);
            return;
        }
        UnifiedResponse::Pps(pps) => {
            trace!("Got GPS pps: {:?}", pps);
            parse_pps(pps, time)
        }
        UnifiedResponse::Gst(gst) => {
            trace!("Got GPS Gst: {:?}", gst);
            return;
        }
        UnifiedResponse::Att(att) => {
            trace!("Got GPS att: {:?}", att);
            return;
        }
        UnifiedResponse::Imu(att) => {
            trace!("Got GPS imu: {:?}", att);
            return;
        }
        UnifiedResponse::Toff(pps) => {
            trace!("Got GPS pps: {:?}", pps);
            return;
        }
        UnifiedResponse::Osc(osc) => {
            trace!("Got GPS osc: {:?}", osc);
            return;
        }
        UnifiedResponse::Poll(poll) => {
            trace!("Got GPS poll: {:?}", poll);
            return;
        }
        UnifiedResponse::Subframe(value) => {
            trace!("Got GPS subframe: {:?}", value);
            return;
        }
        e => {
            warn!("Unknown message: {:?}", e);
            return;
        }
    };

    for msg in msgs {
        if let Err(e) = mqtt_sender_tx.send(msg).await {
            warn!("Error sending GPS message: {}", e);
        }
    }
}

const MODE: &str = "TPU/GPS2/Mode";
const SPEED: &str = "TPU/GPS2/GroundSpeed";
const COORDS: &str = "TPU/GPS2/Location";
const ALT: &str = "TPU/GPS2/Altitude";
fn parse_tpv(tpv: Tpv, time: u64) -> Vec<PublishableMessage> {
    let mut ret = vec![];

    ret.push(PublishableMessage {
        topic: MODE.to_string(),
        data: vec![tpv.mode as usize as f32],
        unit: "enum",
        time,
    });

    if tpv.speed.is_some() {
        ret.push(PublishableMessage {
            topic: SPEED.to_string(),
            data: vec![tpv.speed.unwrap()],
            unit: "knot",
            time,
        });
    }

    if tpv.lat.is_some() && tpv.lon.is_some() {
        ret.push(PublishableMessage {
            topic: COORDS.to_string(),
            data: vec![tpv.lat.unwrap() as f32, tpv.lon.unwrap() as f32],
            unit: "coordinate",
            time,
        });
    }

    if tpv.alt_hae.is_some() {
        ret.push(PublishableMessage {
            topic: ALT.to_string(),
            data: vec![tpv.alt_hae.unwrap()],
            unit: "meter",
            time,
        });
    }
    ret
}

const PPS: &str = "TPU/GPS2/PPS";
fn parse_pps(pps: Pps, time: u64) -> Vec<PublishableMessage> {
    if pps.precision.is_some() {
        vec![PublishableMessage {
            topic: PPS.to_string(),
            data: vec![pps.precision.unwrap()],
            unit: "NTP precison",
            time,
        }]
    } else {
        vec![]
    }
}
