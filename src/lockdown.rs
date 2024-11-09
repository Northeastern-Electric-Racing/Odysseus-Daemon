use std::error::Error;

use tokio::sync::watch::Receiver;
use tracing::info;

pub async fn lockdown_runner(
    mut hv_stat_recv: Receiver<bool>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    loop {
        tokio::select! {
            new = hv_stat_recv.changed() => {
                new?;
                info!("New HV state:{}", *hv_stat_recv.borrow_and_update());
            }
        }
    }
}
