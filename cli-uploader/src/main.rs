use clap::Parser;
use odysseus_uploader::upload_files;

/// ody-visual command line arguments
#[derive(Parser, Debug)]
#[command(version)]
struct UploaderArgs {
    /// The output folder of data (videos, audio, text logs, etc), no trailing slash
    #[arg(short = 'f', long, env = "ODYSSEUS_DAEMON_OUTPUT_FOLDER")]
    output_folder: String,

    /// The URL of Scylla
    #[arg(short = 'u', long, env = "ODYSSEUS_DAEMON_SCYLLA_URL")]
    scylla_url: String,

    /// Whether to send logger data
    #[arg(short = 'l', long, env = "ODYSSEUS_DAEMON_SEND_LOGGER_DATA")]
    send_logger: bool,

    /// Whether to send video data
    #[arg(short = 'v', long, env = "ODYSSEUS_DAEMON_SEND_VIDEO_DATA")]
    send_video: bool,

    /// Whether to send serial data
    #[arg(short = 's', long, env = "ODYSSEUS_DAEMON_SEND_SERIAL_DATA")]
    send_serial: bool,
}

#[tokio::main]
async fn main() {
    let cli = UploaderArgs::parse();

    let thread = upload_files(
        &cli.output_folder,
        &cli.scylla_url,
        cli.send_logger,
        cli.send_video,
        cli.send_serial,
    );

    thread.await.expect("Upload failed");

    println!(
        "Done, feel free to clear the inside of the {} directory!",
        cli.output_folder
    );
}
