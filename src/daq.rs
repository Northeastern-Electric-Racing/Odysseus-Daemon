use std::time::{Duration, UNIX_EPOCH};

use tokio::{
    io::{AsyncBufReadExt, BufReader},
    sync::mpsc::Sender,
};
use tokio_serial::{SerialPortBuilderExt, SerialStream};
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};

use socketcan::{CanFrame, EmbeddedFrame, StandardId};

use crate::PublishableMessage;

const CAN_ID: u16 = 0x630;

pub async fn collect_daq(
    cancel_token: CancellationToken,
    device: String,
    daq_monitor_tx: Sender<bool>,
    mqtt_sender_tx: Sender<PublishableMessage>,
    can_handler_tx: Sender<CanFrame>,
) {
    let port = tokio_serial::new(device, 115_200)
        .open_native_async()
        .expect("Failed to open port");

    let reader = BufReader::<SerialStream>::new(port);

    let mut lines = reader.lines();

    loop {
        let (mqtt_msgs, can_msgs) = tokio::select! {
            _ = cancel_token.cancelled() => {
                debug!("Shutting down DAQ process: cancel called");
                break;
            },
            _ = tokio::time::sleep(Duration::from_millis(150)) => {
                warn!("Shutting down DAQ process: 150ms has passed without line");
                break;
            },
            line = lines.next_line() => {
                // first go until $
                let time = UNIX_EPOCH.elapsed().unwrap().as_micros() as u64;
                let line: String = match line {
                    Ok(res) => {
                        match res {
                                Some(res) => {
                                res
                            },
                            None =>  {
                            debug!("Failed to read DAQ buffer no line");
                            continue;
                            }
                        }
                    },
                    Err(error) =>  {
                        debug!("Failed to read DAQ buffer: {}", error);
                        continue;
                    }
                };

                // split up the points
                let res: Vec<_> = line.split('$').next_back().unwrap_or("").split(',').collect();
                if res.len() < 10 {
                    warn!("Under found samples: {}", line);
                    continue;
                }

                // clean up the points
                let mut clean_res: Vec<u64> = Vec::new();
                for item in res {
                    match item.parse::<u64>() {
                        // the inches conversion
                        Ok(val) => clean_res.push(val),
                        Err(e) => warn!("Invalid byte from DAQ: {}", e),
                    }
                }

                (vec![PublishableMessage {
                    topic:"TPU/DAQ/Shockpots".to_string(),
                data:vec![conv_shock(*clean_res.get(1).unwrap()),
                conv_shock(*clean_res.get(2).unwrap()),conv_shock(*clean_res.get(3).unwrap()), conv_shock(*clean_res.get(7).unwrap())], unit: "in",
            time},
                    PublishableMessage { topic: "TPU/DAQ/SteringAngle".to_string(), data: vec![ conv_wheel(*clean_res.get(6).unwrap())], unit: "V", time }
            ], vec![CanFrame::new(StandardId::new(CAN_ID).expect("Failed to create standard id!"),
                &(conv_wheel(*clean_res.get(6).unwrap())).to_be_bytes()).expect("Failed to create CAN frame!")])
            } // NOTE: CAN Frame currently only sends wheel sensor data in big endian
        };

        if !mqtt_msgs.is_empty() {
            if let Err(err) = daq_monitor_tx.send(true).await {
                warn!("Failed to send to daq watchdog: {}", err);
            };
        }
        for mqtt in mqtt_msgs {
            if let Err(err) = mqtt_sender_tx
                .send_timeout(mqtt, Duration::from_millis(50))
                .await
            {
                warn!("Could not pub to sender from daq: {}", err);
            }
        }

        for can_frame in can_msgs {
            if let Err(err) = can_handler_tx
                .send_timeout(can_frame, Duration::from_millis(50))
                .await
            {
                warn!("Could not pub to can senser from daq {}", err);
            }
        }
    }
}

fn conv_shock(val: u64) -> f32 {
    (val as f32 / 4095.0) * 54.44 * (1.0 / 25.4)
}

fn conv_wheel(val: u64) -> f32 {
    (val as f32 / 4095.0) * 3.3
}
