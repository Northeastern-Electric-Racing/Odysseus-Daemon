use std::error::Error;

use tokio::sync::watch::Receiver;
use tokio_util::sync::CancellationToken;

/// runs the mute/unmute functionality
pub async fn audible_manager(
    cancel_token: CancellationToken,
    mut mute_stat_recv: Receiver<bool>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    loop {
        tokio::select! {
            _ = cancel_token.cancelled() => {

            },
            new = mute_stat_recv.changed() => {
                new?;

            }
        }
    }
}
