use std::{path::PathBuf, time::Duration, time::UNIX_EPOCH};
use tracing::trace;

use tokio::{sync::mpsc::Sender, time::Instant};
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};

use crate::PublishableMessage;

/// The path of the measurement in sysfs
/// the message to publish
/// the last instant and value it was measured, or None if its a simple read
type NetMeasurement = (PathBuf, PublishableMessage, Option<(Instant, f32)>);

pub async fn network_scraper(
    cancel_token: CancellationToken,
    mqtt_sender_tx: Sender<PublishableMessage>,
    base_name: String,
    network_ifaces: Vec<String>,
) {
    let mut sync_timer = tokio::time::interval(Duration::from_millis(500));
    // the main structure of data to mutate
    let mut send_list: Vec<NetMeasurement> = vec![];

    for iface in network_ifaces {
        let path: PathBuf = PathBuf::from(format!("/sys/class/net/{iface}/statistics/tx_bytes"));

        let msg = PublishableMessage {
            topic: format!("{base_name}/{iface}/tx_bytes"),
            data: vec![],
            unit: "bytes/s",
            time: 0,
        };

        send_list.push((path, msg, Some((Instant::now(), 0f32))));

        let path: PathBuf = PathBuf::from(format!("/sys/class/net/{iface}/statistics/rx_bytes"));

        let msg = PublishableMessage {
            topic: format!("{base_name}/{iface}/rx_bytes"),
            data: vec![],
            unit: "bytes/s",
            time: 0,
        };

        send_list.push((path, msg, Some((Instant::now(), 0f32))));

        let path: PathBuf = PathBuf::from(format!("/sys/class/net/{iface}/statistics/rx_errors"));

        let msg = PublishableMessage {
            topic: format!("{base_name}/{iface}/rx_errors"),
            data: vec![],
            unit: "bytes/s",
            time: 0,
        };

        send_list.push((path, msg, None));
    }

    loop {
        tokio::select! {
            _ = cancel_token.cancelled() => {
                debug!("Shutting down color controller!");
                break;
            },
            _ = sync_timer.tick() => {
                if let Err(err) = handle_tick(&mut send_list).await {
                    warn!("Error trying to read net statistics: {}", err);
                    continue;
                }
                handle_sends(&send_list, &mqtt_sender_tx).await;
            }
        }
    }
}

async fn handle_tick(send_list: &mut [NetMeasurement]) -> Result<(), std::io::Error> {
    for item in send_list.iter_mut() {
        let ok = tokio::fs::read_to_string(item.0.clone()).await?;
        let ok = ok.trim();
        trace!("Got val {}", ok);
        let res = ok.parse::<f32>().unwrap_or(-1f32);
        trace!("Got val {}", res);
        item.1.data = if let Some(edit) = item.2.as_mut() {
            let old_time = edit.0;
            edit.0 = Instant::now();
            let old = edit.1;
            edit.1 = res;
            trace!(
                "Debug write {} - {} / {:?} - {:?}",
                edit.1, old, edit.0, old_time
            );
            vec![(edit.1 - old) / (edit.0 - old_time).as_secs() as f32]
        } else {
            vec![res]
        };
        item.1.time = UNIX_EPOCH.elapsed().unwrap().as_micros() as u64;
    }

    Ok(())
}

async fn handle_sends(
    send_list: &Vec<NetMeasurement>,
    mqtt_sender_tx: &Sender<PublishableMessage>,
) {
    for item in send_list {
        if let Err(err) = mqtt_sender_tx.send(item.1.clone()).await {
            warn!("Error putting message in MQTT queue for net: {} ", err);
        }
    }
}
