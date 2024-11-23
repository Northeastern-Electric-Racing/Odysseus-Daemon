use std::error::Error;

use tokio::{process::Command, sync::watch::Receiver};
use tokio_util::sync::CancellationToken;

/// runs the mute/unmute functionality
pub async fn audible_manager(
    cancel_token: CancellationToken,
    mut mute_stat_recv: Receiver<bool>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    loop {
        tokio::select! {
            _ = cancel_token.cancelled() => {
                    Command::new("linphonecsh").args(["generic", "unmute"]).spawn()?.wait().await?;
            },
            new = mute_stat_recv.changed() => {
                new?;
                // to mute or not
                if *mute_stat_recv.borrow_and_update() {
                    Command::new("linphonecsh").args(["generic", "mute"]).spawn()?.wait().await?;
                } else {
                    Command::new("linphonecsh").args(["generic", "unmute"]).spawn()?.wait().await?;
                }
            }
        }
    }
}
