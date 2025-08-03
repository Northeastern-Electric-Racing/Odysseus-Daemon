use socketcan::CanFrame;
use tokio::sync::mpsc::Sender;
use tokio::task::JoinHandle;
use tokio::time::interval;
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};

use crate::daq::collect_daq;
use crate::PublishableMessage;

use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

pub async fn monitor_daq(
    cancel_token: CancellationToken,
    device: String,
    mqtt_sender_tx: Sender<PublishableMessage>,
    can_handler_tx: Sender<CanFrame>,
) {
    let mut timeout = interval(Duration::from_millis(1000));

    let watchdog = AtomicBool::new(false);

    let (daq_monitor_tx, mut daq_monitor_rx) = tokio::sync::mpsc::channel::<bool>(1000);
    let daq_cancel_token = CancellationToken::new();

    let mut task: Option<JoinHandle<()>> = None;

    loop {
        tokio::select! {
            _ = cancel_token.cancelled() => {
                daq_cancel_token.cancel();
                if task.is_some() {
                task.unwrap().await;
                }
                debug!("Shutting down daq monitor");
                break;
            }

            _ = timeout.tick() => {
                if !watchdog.load(Ordering::Relaxed) {
                    daq_cancel_token.cancel();
                    if task.is_some() {
                        task.unwrap().await;
                        task = None;
                    }
                    task = Some(tokio::task::spawn(collect_daq(daq_cancel_token.clone(), device.clone(), daq_monitor_tx.clone(), mqtt_sender_tx.clone(), can_handler_tx.clone())));
                    warn!("Respawing DAQ thread");
                }
                watchdog.store(false, Ordering::Relaxed);
            }

            _ = daq_monitor_rx.recv() => {
                watchdog.store(true, Ordering::Relaxed);
            }
        }
    }
}
