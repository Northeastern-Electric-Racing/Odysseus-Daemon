use std::{
    io::{BufRead, BufReader},
    time::{Duration, UNIX_EPOCH},
};

use serialport::TTYPort;
use tokio::sync::mpsc::Sender;
use tokio_util::sync::CancellationToken;
use tracing::{debug, trace, warn};

use crate::PublishableMessage;

pub async fn collect_daq(
    cancel_token: CancellationToken,
    device: String,
    mqtt_sender_tx: Sender<PublishableMessage>,
) {
    let _ = mqtt_sender_tx;

    let port = serialport::new(device, 115_200)
        .open_native()
        .expect("Failed to open port");

    let mut reader_time = tokio::time::interval(Duration::from_millis(1));
    let mut reader = BufReader::<TTYPort>::new(port);
    let mut buf = String::with_capacity(40);

    loop {
        let msgs = tokio::select! {
            _ = cancel_token.cancelled() => {
                debug!("Shutting down MQTT processor!");
                break;
            },
            _ = reader_time.tick() => {
                buf.clear();
                let time = UNIX_EPOCH.elapsed().unwrap().as_micros() as u64;
                match reader.read_line(&mut buf) {
                    Ok(res) => {
                        trace!("Read {} bytes from DAQ", res);
                    },
                    Err(e) => warn!("Failed to read DAQ buffer: {}", e),
                }
                let res: Vec<_> = buf.split(',').collect();
                let mut clean_res: Vec<f32> = Vec::new();
                for item in res {
                    match item.parse::<f32>() {
                        // the inches conversion
                        Ok(val) => clean_res.push((val / 4095.0) * 54.44 * (1.0/25.4)),
                        Err(e) => warn!("Invalid byte from DAQ: {}", e),
                    }
                }

                if clean_res.len() < 10 {
                    warn!("Under found samples");
                    continue;
                }

                vec![PublishableMessage {
                    topic:"TPU/DAQ/Shockpots".to_string(),
                data:vec![*clean_res.first().unwrap(),
                *clean_res.get(1).unwrap(), *clean_res.get(2).unwrap(), *clean_res.get(3).unwrap()], unit: "in",
            time},
                    PublishableMessage { topic: "TPU/DAQ/StteringAngle".to_string(), data: vec![ *clean_res.get(4).unwrap()], unit: "deg", time }
            ]

            }
        };

        for msg in msgs {
            if let Err(err) = mqtt_sender_tx.send(msg).await {
                warn!("Could not pub to sender from daq: {}", err);
            }
        }
    }
}
