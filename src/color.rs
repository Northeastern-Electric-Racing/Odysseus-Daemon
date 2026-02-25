use palette::{Hsv, IntoColor, LinSrgb, RgbHue, Srgb};
use std::{array, path::PathBuf, str::FromStr};

/**
 * This module is written to optimize speed of translate colorspace and write colorspace functions so they are as efficient as possible.
 */
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};

use crate::playback_data::PlaybackData;

const LED_BANK_SIZE_REAL: usize = 9;
const LED_BANK_SIZE_FUCKED: usize = 12;

type WheelColor = Hsv<palette::encoding::Srgb, f32>;
type Settings = [WheelColor; LED_BANK_SIZE_REAL];

#[derive(Default)]
struct StartupVars {
    pub curr_led: usize,
}

enum WheelMode {
    Startup(StartupVars),
}

const LED_BANK_WRITE_LISTINGS: [(&str, usize); LED_BANK_SIZE_FUCKED] = [
    ("/sys/class/leds/l0:1/", 2),
    ("/sys/class/leds/l2:4/", 3),
    ("/sys/class/leds/l5:6/", 2),
    ("/sys/class/leds/l7:8/", 2),
    ("/sys/class/leds/l9:11/", 3),
    ("/sys/class/leds/l12:13/", 2),
    ("/sys/class/leds/l4:15/", 2),
    ("/sys/class/leds/l16:17/", 2),
    ("/sys/class/leds/m0:1/", 2),
    ("/sys/class/leds/m2:4/", 3),
    ("/sys/class/leds/m5:6/", 2),
    ("/sys/class/leds/m7:8/", 2),
];

/**
 * Translates the color space into a tuple of colors to our fucked up mapping
 * (see altium schematic, table 7-1 on LP5018 PDF, and lp5018a.dts on Odysseus)
 */
fn translate_colorspace(
    data: [(u8, u8, u8); LED_BANK_SIZE_REAL],
) -> Result<[heapless::String<10>; LED_BANK_SIZE_FUCKED], heapless::CapacityError> {
    let ret: [heapless::String<10>; LED_BANK_SIZE_FUCKED] = [
        heapless::String::from_str(format!("{} {}", data[0].1, data[0].0).as_ref())?,
        heapless::String::from_str(format!("{} {} {}", data[0].2, data[1].1, data[1].0).as_ref())?,
        heapless::String::from_str(format!("{} {}", data[1].2, data[2].1).as_ref())?,
        heapless::String::from_str(format!("{} {}", data[2].0, data[2].2).as_ref())?,
        heapless::String::from_str(format!("{} {} {}", data[3].1, data[3].0, data[3].2).as_ref())?,
        heapless::String::from_str(format!("{} {}", data[4].1, data[4].0).as_ref())?,
        heapless::String::from_str(format!("{} {}", data[4].2, data[5].1).as_ref())?,
        heapless::String::from_str(format!("{} {}", data[5].0, data[5].2).as_ref())?,
        heapless::String::from_str(format!("{} {}", data[6].1, data[6].0).as_ref())?,
        heapless::String::from_str(format!("{} {} {}", data[6].2, data[7].1, data[7].0).as_ref())?,
        heapless::String::from_str(format!("{} {}", data[7].2, data[8].1).as_ref())?,
        heapless::String::from_str(format!("{} {}", data[8].0, data[8].2).as_ref())?,
    ];

    Ok(ret)
}

/**
 * Writes the paths of the LED driver to SysFS for color
 */
async fn write_paths(
    data: [heapless::String<10>; LED_BANK_SIZE_FUCKED],
    path_cache: &[PathBuf; LED_BANK_SIZE_FUCKED],
) -> () {
    for item in data.into_iter().zip(path_cache) {
        if let Err(err) = tokio::fs::write(item.1, item.0).await {
            warn!("Could not write to file for color controller! {}", err);
        }
    }
}

async fn execute_brightness_step(brightness: u8, path_cache: &[PathBuf; LED_BANK_SIZE_FUCKED]) {
    write_paths(
        array::repeat(heapless::String::from_str(&brightness.to_string()).unwrap()),
        path_cache,
    )
    .await;
}

/**
 * Runs a "tick" at which all of the LED settings are asynchronously entered
 */
async fn execute_step(settings: Settings, path_cache: &[PathBuf; LED_BANK_SIZE_FUCKED]) {
    let Ok(res) = translate_colorspace(settings.map(|f| {
        let srgb: Srgb<f32> = f.into_color();
        let lin_rgb_f32: LinSrgb<f32> = srgb.into_color();
        let s2: LinSrgb<u8> = lin_rgb_f32.into_format();
        s2.into_components()
    })) else {
        warn!("Could not translate colorspace for color controller!");
        return;
    };
    write_paths(res, path_cache).await;
}

/*
 * Uses the current mode to calculate the current LED conditions
 */
fn calculate_settings(mode: &mut WheelMode, last_settings: &Settings) -> Settings {
    match mode {
        WheelMode::Startup(startup_vars) => {
            let mut new_settings = *last_settings;
            new_settings[startup_vars.curr_led].hue += RgbHue::from_degrees(1f32);
            if new_settings[startup_vars.curr_led]
                .hue
                .into_positive_degrees()
                > 359.0
            {
                new_settings[startup_vars.curr_led] = Hsv::from_components((0f32, 0f32, 0f32));
                startup_vars.curr_led += 1;
                if startup_vars.curr_led > LED_BANK_SIZE_REAL {
                    startup_vars.curr_led = 0;
                }
            }

            new_settings
        }
    }
}

pub async fn color_controller(
    cancel_token: CancellationToken,
    _mqtt_recv_rx: broadcast::Receiver<PlaybackData>,
) {
    let path_cache: [PathBuf; LED_BANK_SIZE_FUCKED] = LED_BANK_WRITE_LISTINGS.map(|f| {
        PathBuf::from_str(&format!("{}multi_intensity", f.0)).expect("Could not parse path")
    });
    let brightness_cache: [PathBuf; LED_BANK_SIZE_FUCKED] = LED_BANK_WRITE_LISTINGS
        .map(|f| PathBuf::from_str(&format!("{}brightness", f.0)).expect("Could not parse path"));

    // smaller than fastexst human color change (about 13ms)
    let mut tick_size = tokio::time::interval(tokio::time::Duration::from_millis(10));

    let mut current_mode = WheelMode::Startup(StartupVars::default());
    let current_brightness = 175;
    let mut last_settings: Settings = [
        Hsv::new(0.0, 0.0, 0.0),
        Hsv::new(0.0, 0.0, 0.0),
        Hsv::new(0.0, 0.0, 0.0),
        Hsv::new(0.0, 0.0, 0.0),
        Hsv::new(0.0, 0.0, 0.0),
        Hsv::new(0.0, 0.0, 0.0),
        Hsv::new(0.0, 0.0, 0.0),
        Hsv::new(0.0, 0.0, 0.0),
        Hsv::new(0.0, 0.0, 0.0),
    ];

    execute_brightness_step(current_brightness, &brightness_cache).await;

    loop {
        tokio::select! {
            _ = cancel_token.cancelled() => {
                debug!("Shutting down color controller!");
                break;
            },
            _ = tick_size.tick() => {
                let settings = calculate_settings(&mut current_mode, &last_settings);
                execute_step(settings, &path_cache).await;
                last_settings = settings;
            }
        }
    }
}
