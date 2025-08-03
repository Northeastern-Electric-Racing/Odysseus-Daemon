use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use clap::Parser;
use odysseus_daemon::{
    audible::audible_manager,
    can_handler::can_handler,
    daq_monitor::monitor_daq,
    lockdown::lockdown_runner,
    logger::logger_manager,
    mqtt_handler::{MqttProcessor, MqttProcessorOptions},
    numerical::collect_data,
    playback_data,
    sys_parser::sys_parser,
    visual::{run_save_pipeline, SavePipelineOpts},
    HVTransition, PublishableMessage, SAVE_LOCATION,
};
use rumqttc::v5::{mqttbytes::v5::Publish, AsyncClient};
use tokio::{
    signal,
    sync::{mpsc, watch},
};
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use tracing::{info, level_filters::LevelFilter};
use tracing_subscriber::{fmt::format::FmtSpan, EnvFilter};

use socketcan::CanFrame;

/// ody-visual command line arguments
#[derive(Parser, Debug)]
#[command(version)]
struct VisualArgs {
    /// Augment HV on
    #[arg(long, env = "ODYSSEUS_DAEMON_AUGMENT_HV")]
    mock: bool,

    /// Enable lockdown module
    #[arg(short = 's', long, env = "ODYSSEUS_DAEMON_LOCKDOWN_ENABLE")]
    lockdown: bool,

    /// USB device ports (as seen on usbip list -l) to lock down
    #[arg(long, env = "ODYSSEUS_DAEMON_LOCKDOWN_USBS", num_args = 1..4)]
    usbs_locked: Option<Vec<String>>,

    /// Enable audio module
    #[arg(short = 'a', long, env = "ODYSSEUS_DAEMON_AUDIBLE_ENABLE")]
    audible: bool,

    /// Enable data module
    #[arg(short = 'd', long, env = "ODYSSEUS_DAEMON_DATA_ENABLE")]
    data: bool,

    /// Enable daq module
    #[arg(long, env = "ODYSSEUS_DAEMON_DAQ_ENABLE")]
    daq: bool,

    /// Daq USB device
    #[arg(long, env = "ODYSSEUS_DAEMON_DAQ_DEVICE")]
    daq_device: Option<String>,

    /// Enable logger
    #[arg(long, env = "ODYSSEUS_DAEMON_LOGGER_ENABLE")]
    logger: bool,

    /// Enable video module
    #[arg(short = 'v', long, env = "ODYSSEUS_DAEMON_VIDEO_ENABLE")]
    video: bool,

    /// Enable Mosquitto SYS translator module
    #[arg(long, env = "ODYSSEUS_DAEMON_SYS_ENABLE")]
    sys: bool,

    /// The input video file
    #[arg(short = 'l', long, env = "ODYSSEUS_DAEMON_VIDEO_FILE")]
    video_uri: Option<String>,

    /// The MQTT/Siren URL
    #[arg(
        short = 'u',
        long,
        default_value = "localhost:1883",
        env = "ODYSSEUS_DAEMON_SIREN_URL"
    )]
    mqtt_url: String,

    /// The Scylla URL
    #[arg(short = 'S', long, env = "ODYSSEUS_DAEMON_SCYLLA_URL")]
    scylla_url: String,

    /// The output folder of data (videos, audio, text logs, etc), no trailing slash
    #[arg(short = 'f', long, env = "ODYSSEUS_DAEMON_OUTPUT_FOLDER")]
    output_folder: String,

    /// The SocketCAN interface port
    #[arg(short = 'c', long, env = "SOCKETCAN_IFACE", default_value = "can0")]
    socketcan_iface: String,
}

/// Folder hierarchy
/// Main folder --> specified by the user --output_folder
///                                               |
///                                               |
///                                         event-<TIME_MS>
///                                               |
///                                              / \
/// (video): ner24-frontcam.avi; (logger): data_dump.log; (serial): serial_dump.log; (audio): ner24-comms.mp3
#[tokio::main]
async fn main() {
    let cli = VisualArgs::parse();

    println!("Initializing odysseus daemon...");
    println!("Initializing fmt subscriber");
    // construct a subscriber that prints formatted traces to stdout
    // if RUST_LOG is not set, defaults to loglevel INFO
    let subscriber = tracing_subscriber::fmt()
        .with_thread_ids(true)
        .with_ansi(true)
        .with_thread_names(true)
        .with_span_events(FmtSpan::CLOSE)
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::INFO.into())
                .from_env_lossy(),
        )
        .finish();
    // use that subscriber to process traces emitted after this point
    tracing::subscriber::set_global_default(subscriber).expect("Could not init tracing");

    // set save location
    SAVE_LOCATION.get_or_init(|| cli.output_folder.clone());

    // channel to pass the mqtt data
    // TODO tune buffer size
    let (mqtt_sender_tx, mqtt_sender_rx) = mpsc::channel::<PublishableMessage>(1000);

    let (mqtt_sys_tx, mqtt_sys_rx) = if cli.sys {
        let res = mpsc::channel::<Publish>(100);
        (Some(res.0), Some(res.1))
    } else {
        (None, None)
    };

    let (can_handler_tx, can_handler_rx) = mpsc::channel::<CanFrame>(1000);

    let (hv_stat_send, hv_stat_recv) = watch::channel(HVTransition::TransitionOff);
    let (mute_stat_send, mute_stat_recv) = watch::channel(false);

    // create wildcard mqtt channel only if logger is enabled
    let (mqtt_recv_tx, mqtt_recv_rx) = if cli.logger {
        let (tx, rx) = mpsc::channel::<playback_data::PlaybackData>(1000);
        (Some(tx), Some(rx))
    } else {
        (None, None)
    };

    let task_tracker = TaskTracker::new();
    let token = CancellationToken::new();

    // time is wrong for a while upon boot.  hold on until it is OK
    while !SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .is_ok_and(|time| time > Duration::from_millis(1730247194876))
    {
        info!("Waiting for good time");
        tokio::time::sleep(Duration::from_secs(1)).await;
    }

    info!("Running MQTT processor");
    let (recv, opts) = MqttProcessor::new(
        token.clone(),
        mqtt_sender_rx,
        hv_stat_send,
        cli.mock,
        mute_stat_send,
        mqtt_recv_tx,
        mqtt_sys_tx,
        MqttProcessorOptions {
            mqtt_path: cli.mqtt_url,
            scylla_url: cli.scylla_url,
        },
    );
    let (client, eventloop) = AsyncClient::new(opts, 600);
    let client_sharable: Arc<AsyncClient> = Arc::new(client);
    task_tracker.spawn(recv.process_mqtt(client_sharable.clone(), eventloop));

    // TASK SPAWNING

    info!("Enable CAN handler");
    task_tracker.spawn(can_handler(
        token.clone(),
        cli.socketcan_iface,
        can_handler_rx,
    ));

    if cli.video {
        info!("Running video module");
        task_tracker.spawn(run_save_pipeline(
            token.clone(),
            hv_stat_recv.clone(),
            SavePipelineOpts {
                video: cli
                    .video_uri
                    .expect("Must provide video URI if video is enabled!"),
            },
        ));
    }
    if cli.data {
        info!("Running TPU data collector");
        task_tracker.spawn(collect_data(token.clone(), mqtt_sender_tx.clone()));
    }
    if cli.daq {
        info!("Running DAQ data collector");
        task_tracker.spawn(monitor_daq(
            token.clone(),
            cli.daq_device.expect("failed to init daq device"),
            mqtt_sender_tx.clone(),
            can_handler_tx,
        ));
    }

    if cli.lockdown {
        info!("Running lockdown module");
        task_tracker.spawn(lockdown_runner(
            token.clone(),
            hv_stat_recv.clone(),
            cli.usbs_locked
                .expect("USBs required when enabling lockdown module"),
        ));
    }

    if cli.audible {
        info!("Running audio module");
        task_tracker.spawn(audible_manager(token.clone(), mute_stat_recv));
    }
    if cli.logger {
        info!("Running logger module");
        task_tracker.spawn(logger_manager(
            token.clone(),
            mqtt_recv_rx.unwrap(),
            hv_stat_recv.clone(),
        ));
    }
    if cli.sys {
        info!("Running SYS translator");
        task_tracker.spawn(sys_parser(
            token.clone(),
            mqtt_sys_rx.unwrap(),
            mqtt_sender_tx,
        ));
    }

    task_tracker.close();

    info!("Initialization complete, ready...");
    info!("Use Ctrl+C or SIGINT to exit cleanly!");

    // listen for ctrl_c, then cancel, close, and await for all tasks in the tracker.  Other tasks cancel vai the default tokio system
    signal::ctrl_c()
        .await
        .expect("Could not read cancellation trigger (ctr+c)");
    info!("Received exit signal, shutting down!");
    token.cancel();
    task_tracker.wait().await;
}
