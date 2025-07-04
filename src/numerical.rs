use std::{fs, time::Duration};

use sysinfo::{Components, MemoryRefreshKind, Pid, ProcessesToUpdate, System};
use tokio::sync::mpsc::Sender;
use tokio_util::sync::CancellationToken;
use tracing::{debug, trace, warn};

use crate::PublishableMessage;

/// sender of the messages
pub async fn collect_data(
    cancel_token: CancellationToken,
    mqtt_sender_tx: Sender<PublishableMessage>,
) {
    // create requisites
    let mut sys = System::new_all();
    sys.refresh_all();

    // for CPU temp
    let mut components = Components::new_with_refreshed_list();
    let mut temperature_component = components
        .iter_mut()
        .find(|x| x.label() == "coretemp Core 0");

    // for broker CPU
    let mut pid = match fs::read_to_string("/var/run/mosquitto.pid") {
        Ok(p) => p,
        Err(_) => {
            warn!("Could not read mosquitto PID, using 1");
            "1".to_string()
        }
    };
    if pid.ends_with('\n') {
        pid.pop();
    }
    let pid_clean = str::parse::<u32>(&pid).unwrap_or_else(|a| {
        warn!("Could not parse mosquitto pid, using 1, error: {}", a);
        1
    });

    // STEP 1: add a refresh rates for the message

    // on board
    let mut cpu_temp_int = tokio::time::interval(Duration::from_secs(2));
    let mut cpu_usage_int = tokio::time::interval(Duration::from_millis(300)); // minimum 200ms. both broker and global
    let mut mem_avail_int = tokio::time::interval(Duration::from_secs(1));

    loop {
        let msgs = tokio::select! {
            _ = cancel_token.cancelled() => {
                debug!("Shutting down MQTT processor!");
                break;
            },
            // STEP 2: add a block to gather the data and send the message, include topic and unit blocks
            _ = cpu_temp_int.tick() => {
                const TOPIC: &str = "TPU/OnBoard/CpuTemp";
                const UNIT: &str = "celsius";

                let value = match temperature_component {
                    Some(ref mut v) => {
                        v.refresh();
                        v.temperature()
                    },
                    None => {
                        warn!("Could not find thermal sensor!");
                        continue;
                    }
                };

                vec![PublishableMessage{ topic: TOPIC.to_string(), data: vec![value.unwrap()], unit: UNIT }]

            }
            _ = cpu_usage_int.tick() => {
                const TOPIC_C: &str = "TPU/OnBoard/CpuUsage";
                const UNIT_C: &str = "%";

                const TOPIC_B: &str = "TPU/OnBoard/BrokerCpuUsage";
                const UNIT_B: &str = "%";


                sys.refresh_cpu_usage();
                sys.refresh_processes(ProcessesToUpdate::Some(&[Pid::from_u32(pid_clean)]),
                        true);

                let process = sys.process(Pid::from_u32(pid_clean)).unwrap_or_else(|| {
                    warn!("Could not find mosquitto from PID, using 1");
                    sys.process(Pid::from(1)).unwrap()
                });
                trace!("Using process: {:?}", process.name());

                vec![
                    PublishableMessage{ topic: TOPIC_C.to_string(), data: vec![sys.global_cpu_usage()], unit: UNIT_C },
                PublishableMessage{ topic: TOPIC_B.to_string(), data: vec![process.cpu_usage()], unit: UNIT_B }]
            },
            _ = mem_avail_int.tick() => {
                const TOPIC: &str = "TPU/OnBoard/MemAvailable";
                const UNIT: &str = "MB";

                sys.refresh_memory_specifics(MemoryRefreshKind::nothing().with_ram());

                vec![PublishableMessage { topic: TOPIC.to_string(), data: vec![sys.free_memory() as f32 / 1e6], unit: UNIT}]
            }
        };
        for msg in msgs {
            let Ok(_) = mqtt_sender_tx.send(msg).await else {
                warn!("Could not send mpsc msg to mqtt send");
                continue;
            };
        }
    }
}
