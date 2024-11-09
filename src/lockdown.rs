use std::error::Error;

use tokio::{process::Command, sync::watch::Receiver};
use tokio_util::sync::CancellationToken;
use tracing::info;

/// Run various HV on/off lockdowns
pub async fn lockdown_runner(
    cancel_token: CancellationToken,
    mut hv_stat_recv: Receiver<bool>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut prev_state = false;

    loop {
        tokio::select! {
            _ = cancel_token.cancelled() => {
                hv_transition_disabled().await;
                break Ok(());
            },
            new = hv_stat_recv.changed() => {
                new?;
                let curr_state = *hv_stat_recv.borrow_and_update();
                if prev_state == curr_state { continue } else{
                    prev_state = curr_state;
                }

                info!("New HV state: {}", curr_state);
                if curr_state {
                    hv_transition_enabled().await;
                } else {
                    hv_transition_disabled().await;
                }

            }
        }
    }
}

/// Transition to HV on
pub async fn hv_transition_enabled() {
    let mut cmd_cerb_dis = Command::new("usbip")
        .args(["unbind", "--busid", "1-1.3"])
        .spawn()
        .unwrap();
    let mut cmd_shep_dis = Command::new("usbip")
        .args(["unbind", "--busid", "1-1.4"])
        .spawn()
        .unwrap();

    cmd_cerb_dis.wait().await.unwrap();
    cmd_shep_dis.wait().await.unwrap();

    // if !cmd_cerb_dis.wait().await.unwrap().success() && !cmd_shep_dis.wait().await.unwrap().success()  {
    //     info!("Failed to run USBIP command(s) to unbind");
    // }
}

/// Transition to HV off
pub async fn hv_transition_disabled() {
    let mut cmd_cerb_rec = Command::new("usbip")
        .args(["bind", "--busid", "1-1.3"])
        .spawn()
        .unwrap();
    let mut cmd_shep_rec = Command::new("usbip")
        .args(["bind", "--busid", "1-1.4"])
        .spawn()
        .unwrap();

    cmd_cerb_rec.wait().await.unwrap();
    cmd_shep_rec.wait().await.unwrap();

    // if !cmd_cerb_rec.wait().await.unwrap().success() && !cmd_shep_rec.wait().await.unwrap().success()  {
    //     println!("Failed to run USBIP command(s) to unbind");
    // }
}
