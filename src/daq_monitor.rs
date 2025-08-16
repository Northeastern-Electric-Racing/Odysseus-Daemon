use socketcan::CanFrame;
use tokio::sync::mpsc::Sender;
use tokio::task::JoinHandle;
use tokio::time::interval;
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};

use crate::daq::collect_daq;
use crate::PublishableMessage;

use std::time::Duration;

/**
 * The Jack DAQ is prone to freezing, partially due to MQTT issues, but mostly serial piping
 * This code is not production quality as Jack DAQ is not to be used at competition
 */
pub async fn monitor_daq(
    cancel_token: CancellationToken,
    device: String,
    mqtt_sender_tx: Sender<PublishableMessage>,
    can_handler_tx: Sender<CanFrame>,
) {
    let mut timeout = interval(Duration::from_millis(200));

    let mut watchdog = false;

    let (daq_monitor_tx, mut daq_monitor_rx) = tokio::sync::mpsc::channel::<bool>(1000);
    let mut daq_cancel_token = CancellationToken::new();

    let mut task: JoinHandle<()> = tokio::task::spawn(collect_daq(
        daq_cancel_token.clone(),
        device.clone(),
        daq_monitor_tx.clone(),
        mqtt_sender_tx.clone(),
        can_handler_tx.clone(),
    ));

    loop {
        tokio::select! {
            _ = cancel_token.cancelled() => {
                daq_cancel_token.cancel();
                let _ = task.await;
                debug!("Shutting down daq monitor");
                break;
            }

            _ = timeout.tick() => {
                if !watchdog {
                    daq_cancel_token.cancel();
                    match tokio::time::timeout(Duration::from_secs(1), task).await {
                        Ok(_) => (),
                        Err(_) => {
                            warn!("Could not cancel the DAQ task!");
                        },
                    }
                    daq_cancel_token = CancellationToken::new();
                    task = tokio::task::spawn(collect_daq(daq_cancel_token.clone(), device.clone(), daq_monitor_tx.clone(), mqtt_sender_tx.clone(), can_handler_tx.clone()));
                    warn!("Respawing DAQ thread");
                }
                watchdog = false;
            }

            _ = daq_monitor_rx.recv() => {
                watchdog = true;
            }
        }
    }
}
