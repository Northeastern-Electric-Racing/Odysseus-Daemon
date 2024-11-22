use std::error::Error;

use protobuf::Message;
use tokio::sync::mpsc::Receiver;
use tokio_util::sync::CancellationToken;
use tracing::warn;

use crate::playback_data;

/// runs the mute/unmute functionality
/// Takes in a receiver of all MQTT messages
pub async fn logger_manager(
    cancel_token: CancellationToken,
    mut mqtt_recv_rx: Receiver<playback_data::PlaybackData>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    loop {
        tokio::select! {
            _ = cancel_token.cancelled() => {
                return Ok(())
            },
            msg = mqtt_recv_rx.recv() => {
                let parsed_msg = match msg  {
                    Some(msg) => {
                        msg
                    },
                    None => {
                        warn!("Could not receive message!");
                        continue;
                    }
                };
                //println!("{:?}", parsed_msg.write_to_bytes());
            }
        }
    }
}
