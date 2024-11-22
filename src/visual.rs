use std::{error::Error, process::Stdio, sync::Arc, time::Duration};

use tokio::{
    process::{Child, Command},
    sync::{watch::Receiver, RwLock},
};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::HVTransition;

pub struct SavePipelineOpts {
    /// the dev to read for video
    pub video: String,
    /// the folder to save to
    pub save_location: String,
}

/// Run a save pipeline on the items
pub async fn run_save_pipeline(
    cancel_token: CancellationToken,
    mut hv_stat_recv: Receiver<HVTransition>,
    vid_opts: SavePipelineOpts,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    // ffmpeg -f video4linux2 -input_format mjpeg -s 1280x720 -i /dev/video0 -vf "drawtext=fontfile=FreeSerif.tff: \
    //text='%{localtime\:%T}': fontcolor=white@0.8: x=7: y=700" -vcodec libx264 -preset veryfast -f mp4 -pix_fmt yuv420p -y output.mp4

    //let mut res: Option<Child> = None;

    let cmd: Arc<RwLock<Child>> = Arc::new(RwLock::new(
        Command::new("sleep")
            .args(["2147483647"])
            .stdin(Stdio::null())
            .spawn()?,
    ));

    //let mut first_run = false;

    loop {
        tokio::select! {
            _ = cancel_token.cancelled() => {
                info!("Ffmpeg canceled");
                (*cmd.write().await).wait().await.unwrap();
                return Ok(())
            },
            new = hv_stat_recv.changed() => {
                new?;
                let curr_data = *hv_stat_recv.borrow_and_update();
            match curr_data {
                HVTransition::TransitionOn(hvon_data) => {
                    let save_location = format!(
                        "{}/event-{}/ner24-frontcam.avi",
                        vid_opts.save_location,
                        hvon_data.time_ms
                    );
                    info!("Creating and launching ffmpeg...");
                    let cmd_new = Command::new("ffmpeg").args([
                        "-f",
                     "video4linux2",
                        "-input_format",
                    "mjpeg",
                     "-s",
                 "1280x720",
                       "-i",
                   &vid_opts.video,
                   "-vf",
                   r"drawtext=fontfile=FreeSerif.tff: \text='%{localtime\:%F %r}': fontcolor=white@0.8: x=7: y=700",
                  "-vcodec",
                   "libx264",
                   "-preset",
                 "veryfast",
                   "-f",
                   "avi",
                  "-pix_fmt",
                  "yuv420p",
                    "-y",
                   &save_location
                    ]).stdin(Stdio::null()).spawn()?;
                    *cmd.write().await = cmd_new;

                },
                    HVTransition::TransitionOff => {
                        let proc_id = cmd.read().await.id().unwrap_or_default();
                        // logic to safely shutdown since ffmpeg doesnt capture our fake ctrl+c from mqtt
                        // first try and run a SIGTERM kill
                        if let Ok(mut child) = Command::new("kill").args(["-SIGTERM".to_string(), proc_id.to_string(), ]).spawn() {
                            if let Err(err) = child.wait().await {
                                warn!("Failed to gracefully kill ffmpeg: {}", err);
                            }
                        } else {
                            warn!("Failed to gracefully kill ffmpeg.");
                        }
                        // then wait 12 seconds, each second check if the process exited
                        let mut tics_cnt = 0;
                        while tics_cnt < 12 {
                            tokio::time::sleep(Duration::from_secs(1)).await;
                            if (*cmd.write().await).try_wait().is_ok_and(|f| f.is_some()){
                                break;
                            }
                            tics_cnt += 1;
                        }
                        // finally as a last sanity check kill the ffmpeg process, as worst case is leaving it dangling
                        let _ = (*cmd.write().await).kill().await;
                 },
                }
            },
        }
    }
}
