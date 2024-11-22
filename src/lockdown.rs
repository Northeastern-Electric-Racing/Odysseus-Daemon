use std::error::Error;

use tokio::{io, process::Command, sync::watch::Receiver};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::HVTransition;

/// Run various HV on/off lockdowns
/// Takes in a receiver of HV state
pub async fn lockdown_runner(
    cancel_token: CancellationToken,
    mut hv_stat_recv: Receiver<HVTransition>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    loop {
        tokio::select! {
            _ = cancel_token.cancelled() => {
                if let Err(err) = hv_transition_disabled().await {
                    warn!("Could not unlock!!! {}", err);
                }
                break Ok(());
            },
            new = hv_stat_recv.changed() => {
                new?;
                let curr_data = *hv_stat_recv.borrow_and_update();
                let curr_state = match curr_data {
                    HVTransition::TransitionOn(_) => true,
                    HVTransition::TransitionOff => false,
                };
                if curr_state {
                    info!("Locking down!");
                    if let Err(err) = hv_transition_enabled().await {
                        warn!("Could not lock down!!! {}", err);
                    }
                } else {
                    info!("Unlocking!");
                    if let Err(err) = hv_transition_disabled().await {
                        warn!("Could not unlock!!! {}", err);
                    }
                }

            }
        }
    }
}

/// Transition to HV on
pub async fn hv_transition_enabled() -> io::Result<()> {
    let mut cmd_cerb_dis = Command::new("usbip")
        .args(["unbind", "--busid", "1-1.3"])
        .spawn()?;
    let mut cmd_shep_dis = Command::new("usbip")
        .args(["unbind", "--busid", "1-1.4"])
        .spawn()?;

    cmd_cerb_dis.wait().await?;
    cmd_shep_dis.wait().await?;

    // if !cmd_cerb_dis.wait().await.unwrap().success() && !cmd_shep_dis.wait().await.unwrap().success()  {
    //     info!("Failed to run USBIP command(s) to unbind");
    // }
    Ok(())
}

/// Transition to HV off
pub async fn hv_transition_disabled() -> io::Result<()> {
    let mut cmd_cerb_rec = Command::new("usbip")
        .args(["bind", "--busid", "1-1.3"])
        .spawn()?;
    let mut cmd_shep_rec = Command::new("usbip")
        .args(["bind", "--busid", "1-1.4"])
        .spawn()?;

    cmd_cerb_rec.wait().await?;
    cmd_shep_rec.wait().await?;

    // if !cmd_cerb_rec.wait().await.unwrap().success() && !cmd_shep_rec.wait().await.unwrap().success()  {
    //     println!("Failed to run USBIP command(s) to unbind");
    // }
    Ok(())
}
