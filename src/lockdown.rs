use std::{error::Error, process::Stdio, sync::Arc, time::Duration};

use tokio::{
    io,
    process::{Child, Command},
    sync::{watch::Receiver, RwLock},
};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::{HVTransition, SAVE_LOCATION};

/// Run various HV on/off lockdowns
/// Takes in a receiver of HV state
pub async fn lockdown_runner(
    cancel_token: CancellationToken,
    mut hv_stat_recv: Receiver<HVTransition>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let cmds: Arc<RwLock<(Child, Child)>> = Arc::new(RwLock::new((
        Command::new("sleep")
            .args(["2147483647"])
            .stdin(Stdio::null())
            .spawn()?,
        Command::new("sleep")
            .args(["2147483647"])
            .stdin(Stdio::null())
            .spawn()?,
    )));
    if let Err(err) = hv_transition_disabled(&mut (*cmds.write().await)).await {
        warn!("Could not unlock!!! {}", err);
    }

    loop {
        tokio::select! {
            _ = cancel_token.cancelled() => {
                if let Err(err) = hv_transition_disabled(&mut (*cmds.write().await)).await {
                    warn!("Could not unlock!!! {}", err);
                }
                break Ok(());
            },
            new = hv_stat_recv.changed() => {
                new?;
                let curr_data = *hv_stat_recv.borrow_and_update();
                 match curr_data {
                    HVTransition::TransitionOn(hvon_data) => {
                        info!("Locking down!");
                        let Ok(children) = hv_transition_enabled(hvon_data.time_ms).await else {
                            warn!("Could not lock down!!!");
                            continue;
                        };
                        *cmds.write().await = children;
                    },
                    HVTransition::TransitionOff => {
                        info!("Unlocking!");
                    if let Err(err) = hv_transition_disabled(&mut (*cmds.write().await)).await {
                        warn!("Could not unlock!!! {}", err);
                    }
                    },
                }

            }
        }
    }
}

/// Transition to HV on
pub async fn hv_transition_enabled(time_ms: u64) -> io::Result<(Child, Child)> {
    // unbind from the usbipd server
    // this automatically brings back
    let mut cmd_cerb_dis = Command::new("usbip")
        .args(["unbind", "--busid", "1-1.3"])
        .spawn()?;
    let mut cmd_shep_dis = Command::new("usbip")
        .args(["unbind", "--busid", "1-1.4"])
        .spawn()?;

    cmd_cerb_dis.wait().await?;
    cmd_shep_dis.wait().await?;

    tokio::time::sleep(Duration::from_secs(2)).await;

    let mut cmd_cerb_conf = Command::new("stty")
        .args(["-F", "/dev/ttyCerberus", "115200"])
        .spawn()?;

    let mut cmd_shep_conf = Command::new("stty")
        .args(["-F", "/dev/ttyShepherd", "115200"])
        .spawn()?;

    cmd_cerb_conf.wait().await?;
    cmd_shep_conf.wait().await?;

    // TODO actually write the tty read from cat into a file, and

    // if !cmd_cerb_dis.wait().await.unwrap().success() && !cmd_shep_dis.wait().await.unwrap().success()  {
    //     info!("Failed to run USBIP command(s) to unbind");
    // }
    let cerb_save_loc = format!(
        "{}/event-{}/cerberus-dump.cap",
        SAVE_LOCATION.get().unwrap(),
        time_ms
    );
    let shep_save_loc = format!(
        "{}/event-{}/shepherd-dump.cap",
        SAVE_LOCATION.get().unwrap(),
        time_ms
    );
    Ok((
        Command::new("minicom")
            .args([
                "/dev/ttyCerberus",
                "-O",
                "timestamp=extended",
                "-C",
                &cerb_save_loc,
            ])
            .spawn()?,
        Command::new("minicom")
            .args([
                "/dev/ttyCerberus",
                "-O",
                "timestamp=extended",
                "-C",
                &shep_save_loc,
            ])
            .spawn()?,
    ))
}

/// Transition to HV off
pub async fn hv_transition_disabled(child_writers: &mut (Child, Child)) -> io::Result<()> {
    let mut cmd_cerb_rec = Command::new("usbip")
        .args(["bind", "--busid", "1-1.3"])
        .spawn()?;
    let mut cmd_shep_rec = Command::new("usbip")
        .args(["bind", "--busid", "1-1.4"])
        .spawn()?;

    cmd_cerb_rec.wait().await?;
    cmd_shep_rec.wait().await?;

    child_writers.0.kill().await?;
    child_writers.1.kill().await?;

    // if !cmd_cerb_rec.wait().await.unwrap().success() && !cmd_shep_rec.wait().await.unwrap().success()  {
    //     println!("Failed to run USBIP command(s) to unbind");
    // }
    Ok(())
}
