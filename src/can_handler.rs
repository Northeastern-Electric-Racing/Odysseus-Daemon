use tokio::sync::mpsc::Receiver;
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};

use socketcan::{tokio::CanSocket, CanFrame};

pub async fn can_handler(
    cancel_token: CancellationToken, 
    can_interface: String,
    mut can_recv: Receiver<CanFrame>
) {
    let socket = CanSocket::open(&can_interface).expect("Failed to open CAN socket");
    
    loop {
        tokio::select! {
            _ = cancel_token.cancelled() => {
                    debug!("Shutting down CAN handler!");
                    break;
                },
            Some(frame) = can_recv.recv() => {
                match socket.write_frame(frame).await {
                    Ok(_) => (),
                    Err(r) => warn!("Could not send CAN frame: {}", r),
                }
            }
        }
    }
}