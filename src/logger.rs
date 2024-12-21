use std::error::Error;

use protobuf::Message;
use tokio::{
    fs::File,
    io::{AsyncWriteExt, BufWriter},
};
use tokio_util::sync::CancellationToken;
use tracing::warn;

use crate::{playback_data, HVTransition, SAVE_LOCATION};

/// runs the mute/unmute functionality
/// Takes in a receiver of all MQTT messages
pub async fn logger_manager(
    cancel_token: CancellationToken,
    mut mqtt_recv_rx: tokio::sync::mpsc::Receiver<playback_data::PlaybackData>,
    mut hv_stat_recv: tokio::sync::watch::Receiver<HVTransition>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut writer: Option<BufWriter<File>> = None;

    loop {
        tokio::select! {
            _ = cancel_token.cancelled() => {
                if let Some(writer) = writer.as_mut() {
                    return Ok(writer.flush().await?)
                }
                return Ok(())
            },
            new = hv_stat_recv.changed() => {
              new?;
              let val = *hv_stat_recv.borrow_and_update();
              match val {
                  HVTransition::TransitionOn(hvon_data) => {
                        let filename = format!("{}/event-{}/data_dump.log", SAVE_LOCATION.get().unwrap(), hvon_data.time_ms);
                        writer = Some(BufWriter::new(File::create_new(filename).await.expect("Could not create log file!")));
                  },
                  HVTransition::TransitionOff => {
                    if let Some(writ) = writer.as_mut() {
                        writ.flush().await?;
                        writer = None;

                    } else {
                        warn!("Logger - Transition off was unexpected");
                    }
                  },
              }

            },
            msg = mqtt_recv_rx.recv() => {
                if writer.is_none() {
                    continue;
                }

                match msg  {
                    Some(msg) => {
                        if let Some(writ) = writer.as_mut() {
                            if let Err(err) = writ.write(&msg.write_length_delimited_to_bytes().unwrap()).await {
                                warn!("Could not write to log! {}", err);
                            }
                        }
                    },
                    None => {
                        warn!("Could not receive message!");
                        continue;
                    }
                }
                //println!("{:?}", parsed_msg.write_to_bytes());
            }
        }
    }
}
