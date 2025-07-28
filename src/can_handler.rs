use tokio::sync::mpsc::Receiver;
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};

use socketcan::{tokio::CanSocket, CanFrame, StandardId, EmbeddedFrame};

pub async fn can_handler(
    cancel_token: CancellationToken, 
    can_interface: String,
    mut can_recv: Receiver<u8>
) {
    let socket = CanSocket::open(&can_interface).expect("Failed to open CAN socket");
    // how do we get the CAN id?
    let id: StandardId = socketcan::StandardId::new(16).expect("Failed to create standard id!");
    loop {
        tokio::select! {
            _ = cancel_token.cancelled() => {
                    debug!("Shutting down CAN handler!");
                    break;
                },
            Some(msg) = can_recv.recv() => {
                match CanFrame::new(id, &[msg]) {
                    Some(packet) => {
                        match socket.write_frame(packet).await {
                            Ok(_) => (),
                            Err(err) => warn!("Error sending can message {}", err),
                        };
                    }   
                    None => {
                        warn!("Failed to send CAN Frame");
                    }
                }
            }
        }
    }
}