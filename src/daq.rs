use std::{
    io::{BufRead, BufReader},
    time::{Duration, UNIX_EPOCH},
};

use serialport::TTYPort;
use tokio::sync::mpsc::Sender;
use tokio_util::sync::CancellationToken;
use tracing::{debug, trace, warn};

use socketcan::{CanFrame, EmbeddedFrame, StandardId};

use crate::PublishableMessage;

const CAN_ID: u16 = 0x630;

pub async fn collect_daq(
    cancel_token: CancellationToken,
    device: String,
    mqtt_sender_tx: Sender<PublishableMessage>,
    can_handler_tx: Sender<CanFrame>,
) {
    let _ = mqtt_sender_tx;

    let port = serialport::new(device, 115_200)
        .open_native()
        .expect("Failed to open port");

    let mut reader_time = tokio::time::interval(Duration::from_millis(1));
    let mut reader = BufReader::<TTYPort>::new(port);
    let mut buf = String::with_capacity(40);

    loop {
        let (mqtt_msgs, can_msgs) = tokio::select! {
            _ = cancel_token.cancelled() => {
                debug!("Shutting down MQTT processor!");
                break;
            },
            _ = reader_time.tick() => {
                buf.clear();
                // first go until $
                match reader.skip_until(0x24) {
                    Ok(res) => {
                        trace!("Read {} garbage bytes from DAQ", res);
                    },
                    Err(_) =>  {
                        //trace!("Failed to read DAQ buffer: {}", e);
                        continue;
                     }
                }
                // then busy poll until we get the full data, with rl lockout
                let time: u64 = loop {
                     match reader.read_line(&mut buf) {
                        Ok(res) => {
                            trace!("Read {} bytes from DAQ", res);
                            break UNIX_EPOCH.elapsed().unwrap().as_micros() as u64;
                         },
                         Err(_) =>  {
                             //trace!("Failed to read DAQ buffer: {}", e);
                             continue;
                          }
                     };
                };
                buf.pop(); // get rid of newline
                // split up the points
                let res: Vec<_> = buf.split(',').collect();
                if res.len() < 10 {
                    warn!("Under found samples: {}", buf);
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
                    PublishableMessage { topic: "TPU/DAQ/SteringAngle".to_string(), data: vec![ conv_wheel(*clean_res.get(4).unwrap())], unit: "deg", time }
            ], vec![CanFrame::new(StandardId::new(CAN_ID).expect("Failed to create standard id!"),
                &(conv_wheel(*clean_res.get(4).unwrap())).to_be_bytes()).expect("Failed to create CAN frame!")])
            } // NOTE: CAN Frame currently only sends wheel sensor data in big endian
        };

        for mqtt in mqtt_msgs {
            if let Err(err) = mqtt_sender_tx.send(mqtt).await {
                warn!("Could not pub to sender from daq: {}", err);
            }
        }

        for can_frame in can_msgs {
            if let Err(err) = can_handler_tx.send(can_frame).await {
                warn!("Could not pub to can senser from daq {}", err);
            }
        }
    }
}

fn conv_shock(val: u64) -> f32 {
    (val as f32 / 4095.0) * 54.44 * (1.0 / 25.4)
}

fn conv_wheel(val: u64) -> f32 {
    ((val as f32 / 4095.0) * 3.3 - 0.946) * 32.0
}
