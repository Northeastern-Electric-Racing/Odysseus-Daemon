use std::{error::Error, process::Stdio, time::Duration};

use tokio::{
    io,
    process::{Child, Command},
    sync::watch::Receiver,
};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::{HVTransition, SAVE_LOCATION};

/// Run various HV on/off lockdowns
/// Takes in a receiver of HV state
pub async fn lockdown_runner(
    cancel_token: CancellationToken,
    mut hv_stat_recv: Receiver<HVTransition>,
    usbs: Vec<String>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut cmds: Option<(Child, Child)> = None;
    info!("Unlocking initially!");
    if let Err(err) = hv_transition_disabled(&mut cmds, &usbs).await {
        warn!("Could not unlock!!! {}", err);
    }

    loop {
        tokio::select! {
            _ = cancel_token.cancelled() => {
                if let Err(err) = hv_transition_disabled(&mut cmds, &usbs).await {
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
                        let Ok(children) = hv_transition_enabled(hvon_data.time_ms, &usbs).await else {
                            warn!("Could not lock down!!!");
                            continue;
                        };
                        cmds = Some(children);
                    },
                    HVTransition::TransitionOff => {
                        info!("Unlocking!");
                    if let Err(err) = hv_transition_disabled(&mut cmds, &usbs).await {
                        warn!("Could not unlock!!! {}", err);
                    }
                    },
                }

            }
        }
    }
}

/// Transition to HV on
pub async fn hv_transition_enabled(time_ms: u64, usbs: &Vec<String>) -> io::Result<(Child, Child)> {
    // unbind from the usbipd server
    for item in usbs {
        Command::new("usbip")
            .args(["unbind", "--busid", item])
            .spawn()?
            .wait()
            .await?;
    }

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
                "-D",
                "/dev/ttyCerberus",
                "-O",
                "timestamp=extended",
                "-C",
                &cerb_save_loc,
            ])
            .stdout(Stdio::null())
            .spawn()?,
        Command::new("minicom")
            .args([
                "-D",
                "/dev/ttyCerberus",
                "-O",
                "timestamp=extended",
                "-C",
                &shep_save_loc,
            ])
            .stdout(Stdio::null())
            .spawn()?,
    ))
}

/// Transition to HV off
pub async fn hv_transition_disabled(
    child_writers: &mut Option<(Child, Child)>,
    usbs: &Vec<String>,
) -> io::Result<()> {
    // bind to the usbipd daemon
    for usb in usbs {
        Command::new("usbip")
            .args(["bind", "--busid", usb])
            .spawn()?
            .wait()
            .await?;
    }

    if let Some(child_writers) = child_writers {
        child_writers.0.kill().await?;
        child_writers.1.kill().await?;
    }

    // if !cmd_cerb_rec.wait().await.unwrap().success() && !cmd_shep_rec.wait().await.unwrap().success()  {
    //     println!("Failed to run USBIP command(s) to unbind");
    // }
    Ok(())
}
