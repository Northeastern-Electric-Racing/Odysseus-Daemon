use std::{
    error::Error,
    process::Stdio,
    time::{SystemTime, UNIX_EPOCH},
};

use tokio::process::Command;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

pub struct SavePipelineOpts {
    pub video: String,
    pub save_location: String,
}

/// Run a save pipeline on the items
pub async fn run_save_pipeline(
    cancel_token: CancellationToken,
    vid_opts: SavePipelineOpts,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    // ffmpeg -f video4linux2 -input_format mjpeg -s 1280x720 -i /dev/video0 -vf "drawtext=fontfile=FreeSerif.tff: \
    //text='%{localtime\:%T}': fontcolor=white@0.8: x=7: y=700" -vcodec libx264 -preset veryfast -f mp4 -pix_fmt yuv420p -y output.mp4

    // use the passed in folder
    let save_location = format!(
        "{}/frontcam-{}-ner24.avi",
        vid_opts.save_location,
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis()
    );

    info!("Creating and launching ffmpeg...");
    let mut res = Command::new("ffmpeg").args([
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

    tokio::select! {
        _ = cancel_token.cancelled() => {
            info!("Ffmpeg canceled");
            res.wait().await.unwrap();
              Ok(())
        },
        _ = res.wait() => {
            warn!("Ffmpeg ended early!");
             Ok(())
        }
    }
}
