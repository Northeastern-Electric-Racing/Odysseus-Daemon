use std::{error::Error, process::Stdio, time::Duration};

use tokio::{
    process::{Child, Command},
    sync::watch::Receiver,
};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::{HVTransition, SAVE_LOCATION};

pub struct SavePipelineOpts {
    /// the dev to read for video
    pub video: String,
}

/// Run a save pipeline on the items
pub async fn run_save_pipeline(
    cancel_token: CancellationToken,
    mut hv_stat_recv: Receiver<HVTransition>,
    vid_opts: SavePipelineOpts,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    // ffmpeg -f video4linux2 -input_format mjpeg -s 1280x720 -i /dev/video0 -vf "drawtext=fontfile=FreeSerif.tff: \
    //text='%{localtime\:%T}': fontcolor=white@0.8: x=7: y=700" -vcodec libx264 -preset veryfast -f mp4 -pix_fmt yuv420p -y output.mp4

    let mut cmd: Option<Child> = None;

    loop {
        tokio::select! {
            _ = cancel_token.cancelled() => {
                info!("Ffmpeg canceled");
                if cmd.as_ref().is_some() {
                    cmd.unwrap().wait().await.unwrap();
                }
                return Ok(())
            },
            new = hv_stat_recv.changed() => {
                new?;

                let curr_data = *hv_stat_recv.borrow_and_update();
            match curr_data {
                HVTransition::TransitionOn(hvon_data) => {
                    let save_location = format!(
                        "{}/event-{}/ner24-frontcam.mp4",
                        SAVE_LOCATION.get().unwrap(),
                        hvon_data.time_ms
                    );
                    info!("Creating and launching ffmpeg...");
                    let cmd_new = Command::new("ffmpeg").args([
                        "-f",
                     "v4l2",
                     "-framerate",
                     "-video_size",
                     "640x480",
                       "-i",
                   &vid_opts.video,
                   "-c:v", "libx264", "-b:v", "1600k", "-preset", "ultrafast", "-vf",
                   r#"drawtext=text='%{localtime\:%F %r}':fontcolor='#EE4245': x=0: y=0:fontsize=24'"#,
                   "-x264opts", "keyint=50", "-g", "25", "-pix_fmt", "yuv420p", "-y",
                   &save_location
                    ]).stdin(Stdio::null()).spawn()?;
                    cmd = Some(cmd_new);

                },
                    HVTransition::TransitionOff => {
                        if cmd.is_none() {
                            continue;
                        }
                        if let Some(val) = cmd.as_mut() {
                        let proc_id = val.id().unwrap_or_default();
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
                            if val.try_wait().is_ok_and(|f| f.is_some()){
                                break;
                            }
                            tics_cnt += 1;
                        }
                        // finally as a last sanity check kill the ffmpeg process, as worst case is leaving it dangling
                        let _ = val.kill().await;
                        }

                 },
                }
            },
        }
    }
}
