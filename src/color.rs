//! Controls the LEDs on the Steering Wheel.
//!
//! This module is written to optimize speed of translate colorspace and
//! write colorspace functions so they are as efficient as possible.
//!
//! Requires:
//!  - Getting LED topic signal
//!  - SYSFS interface for LEDs as configured in Odysseus
//!
//! Adding a new LED mode:
//! 1. Add it to `WheelMode`
//! 2. Add it to match in from_settings to give it an official index
//! 3. Add persistent variables you need to access when coding it to a
//!    <Name>Vars variable.  Make sure to derive or impl Default and Debug
//! 4. Code the actual logic in the match in `calculate_settings`
//!
//! Adding a new follower (so the lights follow a specific pattern)
//! 1. Add it to `FollowerItemSettings::from_idex`, giving it its own index
//!    (MQTT `NERO/Control/Mode` selects followers by this index)
//! 2. To expose it as a `--color-mode` CLI/env default (not just reachable
//!    over MQTT), add a variant to `DefaultWheelMode` and match it in
//!    `DefaultWheelMode::into_wheel_mode`, pointing at the index from step 1
//!
//! Adding a new bipolar follower (single signed topic; solid color + LED
//! count driven by sign/magnitude instead of a continuous gradient, e.g.
//! motor current: green while discharging, blue while regenerating)
//! 1. Add it to `BipolarFollowerItemSettings::from_idex`, giving it its own
//!    index (MQTT `NERO/Control/Mode` mode 3 selects it by this index)
//! 2. To expose it as a `--color-mode` CLI/env default, add a variant to
//!    `DefaultWheelMode` and match it in `DefaultWheelMode::into_wheel_mode`

use palette::{Hsv, IntoColor, LinSrgb, RgbHue, Srgb};
use std::fmt::Debug;
use std::time::Duration;
use std::{array, path::PathBuf, str::FromStr};

use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, trace, warn};

use crate::playback_data::PlaybackData;

/// the number of leds
const LED_BANK_SIZE_REAL: usize = 9;
/// the number of sysfs multi-led objects
const LED_BANK_SIZE_FUCKED: usize = 12;
/// the cycle time the color algorithms are recomputed
const CALC_CYCLE_TIME: Duration = Duration::from_millis(10);

/// a color for high level userspace passing off
type WheelColor = Hsv<palette::encoding::Srgb, f32>;
/// the array of colors to make each LED in the banl
type Settings = [WheelColor; LED_BANK_SIZE_REAL];

#[derive(Default, Debug)]
struct Demo1Vars {
    /// the LED we are currently cycling through
    pub curr_led: usize,
}

#[derive(Default, Debug)]
enum Demo2VarsSequence {
    #[default]
    Red = 0,
    Green,
    Blue,
    Off,
}

#[derive(Default, Debug)]
struct Demo2Vars {
    /// The current LED we are cycling through
    pub curr_led: usize,
    /// the color we are currently on
    pub curr_status: Demo2VarsSequence,
    /// when the color was last switched
    pub last_refresh: Option<tokio::time::Instant>,
}

#[derive(Default, Debug, Clone, Copy)]
struct FollowerItemSettings {
    /// the topic name to get the data from
    pub topic: &'static str,
    /// the minimum value to display when ranging color or LEDs
    pub min: f32,
    /// the maximum value to display when ranging color or LEDs
    pub max: f32,
    /// the color range to represent (only used in color mode) (min=>max)
    /// Use a site like https://www.hslpicker.com/#00ff6a to decide this
    pub color_range: (RgbHue, RgbHue),
}

impl FollowerItemSettings {
    // ADD NEW FOLLOWER HERE
    fn from_idex(idex: usize) -> Self {
        match idex {
            0 => FollowerItemSettings {
                topic: "BMS/Status/Temp_Internal",
                min: 10f32,
                max: 60f32,
                // green to red with number increasing
                color_range: (RgbHue::from(-40f32), RgbHue::from_degrees(169f32)),
            },
            1 => FollowerItemSettings {
                topic: "VCU/CarState/Speed",
                min: 0f32,
                max: 100f32,
                color_range: (RgbHue::from_degrees(-40f32), RgbHue::from_degrees(169f32)),
            },
            2 => FollowerItemSettings {
                topic: "NERO/FLAPPYBIRD/SCORE",
                min: 0f32,
                max: 20f32,
                color_range: (RgbHue::from_degrees(-40f32), RgbHue::from_degrees(169f32)),
            },
            3.. => {
                warn!("Invalid follower item: {}, using default", idex);
                FollowerItemSettings {
                    topic: "NERO/FLAPPYBIRD/SCORE",
                    min: 0f32,
                    max: 20f32,
                    color_range: (RgbHue::from_degrees(-40f32), RgbHue::from_degrees(169f32)),
                }
            }
        }
    }
}

#[derive(Default, Debug)]
struct FollowerSettings {
    pub lr: FollowerItemSettings,
    pub color: FollowerItemSettings,

    pub lr_val: f32,
    pub color_val: f32,
}

impl FollowerSettings {
    fn to_follower_settings(a: usize, b: usize) -> FollowerSettings {
        let lr = FollowerItemSettings::from_idex(a);
        let color = FollowerItemSettings::from_idex(b);
        FollowerSettings {
            lr,
            color,

            lr_val: lr.min,
            color_val: color.min,
        }
    }
}

#[derive(Default, Debug, Clone, Copy)]
struct BipolarFollowerItemSettings {
    /// the topic name to get the data from
    pub topic: &'static str,
    /// the magnitude (always positive) at which all LEDs are lit while the value is negative
    pub neg_max: f32,
    /// the magnitude (always positive) at which all LEDs are lit while the value is positive
    pub pos_max: f32,
    /// solid color shown while the value is negative
    pub neg_hue: RgbHue,
    /// solid color shown while the value is positive
    pub pos_hue: RgbHue,
}

impl BipolarFollowerItemSettings {
    // ADD NEW BIPOLAR FOLLOWER HERE
    fn from_idex(idex: usize) -> Self {
        match idex {
            0 => BipolarFollowerItemSettings {
                topic: "DTI/Power/DC_Current",
                neg_max: 75f32,
                pos_max: 180f32,
                neg_hue: RgbHue::from_degrees(240f32), // blue
                pos_hue: RgbHue::from_degrees(120f32), // green
            },
            1.. => {
                warn!("Invalid bipolar follower item: {}, using default", idex);
                BipolarFollowerItemSettings {
                    topic: "DTI/Power/DC_Current",
                    neg_max: 75f32,
                    pos_max: 180f32,
                    neg_hue: RgbHue::from_degrees(240f32),
                    pos_hue: RgbHue::from_degrees(120f32),
                }
            }
        }
    }
}

#[derive(Default, Debug)]
struct BipolarFollowerSettings {
    pub item: BipolarFollowerItemSettings,
    pub val: f32,
}

impl BipolarFollowerSettings {
    fn to_bipolar_follower_settings(idex: usize) -> Self {
        BipolarFollowerSettings {
            item: BipolarFollowerItemSettings::from_idex(idex),
            val: 0f32,
        }
    }
}

#[derive(Debug)]
enum WheelMode {
    /// A hue sweep of HSV on a loop
    Demo1(Demo1Vars),
    /// A RGB cycle through each LED
    Demo2(Demo2Vars),
    /// A follower.  The first extra number defines the follower left to right, the 2nd defines the follower for coloring
    /// See FollowerItemSettings::from_idex()
    Follower(FollowerSettings),
    /// A follower driven by a single signed value: LED count grows outward
    /// from zero by magnitude, color is a solid hue selected by sign
    /// (rather than a continuous gradient). See BipolarFollowerItemSettings::from_idex()
    BipolarFollower(BipolarFollowerSettings),
}

impl WheelMode {
    /// Convert from the MQTT settings to a valid Wheel Mode
    /// If an invalid setting is passed in wheel mode is reset to Demo1
    fn from_settings(value: u8, extra_data: &[f32]) -> Self {
        match value {
            0 => Self::Demo1(Demo1Vars::default()),
            1 => Self::Demo2(Demo2Vars::default()),
            2 => {
                if extra_data.len() < 2 {
                    warn!("Invalid mode, switching to demo 1!");
                    Self::Demo1(Demo1Vars::default())
                } else {
                    // Follower takes in two indexes from FollowerItemSettings to determine reactivity
                    Self::Follower(FollowerSettings::to_follower_settings(
                        *extra_data.first().unwrap() as usize,
                        *extra_data.get(1).unwrap() as usize,
                    ))
                }
            }
            3 => {
                if extra_data.is_empty() {
                    warn!("Invalid mode, switching to demo 1!");
                    Self::Demo1(Demo1Vars::default())
                } else {
                    // BipolarFollower takes in one index from BipolarFollowerItemSettings
                    Self::BipolarFollower(BipolarFollowerSettings::to_bipolar_follower_settings(
                        *extra_data.first().unwrap() as usize,
                    ))
                }
            }
            4.. => {
                warn!("Invalid mode, switching to demo 1!");
                Self::Demo1(Demo1Vars::default())
            }
        }
    }
}

/// The default wheel mode to boot the color controller into, before any
/// `NERO/Control/Mode` command has been received over MQTT.
#[derive(clap::ValueEnum, Clone, Copy, Debug, Default)]
pub enum DefaultWheelMode {
    /// A hue sweep of HSV on a loop
    Demo1,
    /// A RGB cycle through each LED
    Demo2,
    /// A follower reacting to NERO/FlappyBirdScore
    #[default]
    Flappy,
    /// A bipolar follower reacting to DTI/Power/DC_Current
    Power,
}

impl DefaultWheelMode {
    fn into_wheel_mode(self) -> WheelMode {
        match self {
            DefaultWheelMode::Demo1 => WheelMode::Demo1(Demo1Vars::default()),
            DefaultWheelMode::Demo2 => WheelMode::Demo2(Demo2Vars::default()),
            // FlappyBirdScore is follower index 2 -- see FollowerItemSettings::from_idex
            DefaultWheelMode::Flappy => {
                WheelMode::Follower(FollowerSettings::to_follower_settings(2, 2))
            }
            // DTI/Power/DC_Current is bipolar follower index 0 -- see BipolarFollowerItemSettings::from_idex
            DefaultWheelMode::Power => {
                WheelMode::BipolarFollower(BipolarFollowerSettings::to_bipolar_follower_settings(0))
            }
        }
    }
}

/// The registers to write to and how many 8 bit values they take up
const LED_BANK_WRITE_LISTINGS: [(&str, usize); LED_BANK_SIZE_FUCKED] = [
    ("/sys/class/leds/l0:1/", 2),
    ("/sys/class/leds/l2:4/", 3),
    ("/sys/class/leds/l5:6/", 2),
    ("/sys/class/leds/l7:8/", 2),
    ("/sys/class/leds/l9:11/", 3),
    ("/sys/class/leds/l12:13/", 2),
    ("/sys/class/leds/l14:15/", 2),
    ("/sys/class/leds/l16:17/", 2),
    ("/sys/class/leds/m0:1/", 2),
    ("/sys/class/leds/m2:4/", 3),
    ("/sys/class/leds/m5:6/", 2),
    ("/sys/class/leds/m7:8/", 2),
];

/// Translates the color space into a tuple of colors to our fucked up mapping
/// (see altium schematic, table 7-1 on LP5018 PDF, and lp5018a.dts on Odysseus)
fn translate_colorspace(
    data: [(u8, u8, u8); LED_BANK_SIZE_REAL],
) -> Result<[heapless::String<10>; LED_BANK_SIZE_FUCKED], heapless::CapacityError> {
    // hande written because I dont care and its slightly more performant
    let ret: [heapless::String<10>; LED_BANK_SIZE_FUCKED] = [
        heapless::String::from_str(format!("{} {} 0", data[0].1, data[0].0).as_ref())?,
        heapless::String::from_str(format!("{} {} {}", data[0].2, data[1].1, data[1].0).as_ref())?,
        heapless::String::from_str(format!("0 {} {}", data[1].2, data[2].1).as_ref())?,
        heapless::String::from_str(format!("{} {} 0", data[2].0, data[2].2).as_ref())?,
        heapless::String::from_str(format!("{} {} {}", data[3].1, data[3].0, data[3].2).as_ref())?,
        heapless::String::from_str(format!("{} 0 {}", data[4].2, data[4].0).as_ref())?,
        heapless::String::from_str(format!("0 {} {}", data[4].1, data[5].2).as_ref())?,
        heapless::String::from_str(format!("{} {} 0", data[5].0, data[5].1).as_ref())?,
        heapless::String::from_str(format!("{} {} 0", data[6].1, data[6].0).as_ref())?,
        heapless::String::from_str(format!("{} {} {}", data[6].2, data[7].1, data[7].0).as_ref())?,
        heapless::String::from_str(format!("0 {} {}", data[7].2, data[8].1).as_ref())?,
        heapless::String::from_str(format!("{} {} 0", data[8].0, data[8].2).as_ref())?,
    ];

    Ok(ret)
}

/// Writes the data to the given paths
async fn write_paths(
    data: [heapless::String<10>; LED_BANK_SIZE_FUCKED],
    path_cache: &[PathBuf; LED_BANK_SIZE_FUCKED],
) -> () {
    for item in data.into_iter().zip(path_cache) {
        trace!("Color: Writing to file {:?} with {}", item.1, item.0);
        if let Err(err) = tokio::fs::write(item.1, item.0).await {
            warn!("Could not write to file for color controller! {}", err);
        }
    }
}

/// Writes to the path given a identical value for each and every path.
/// Useful for setting brightness
async fn execute_brightness_step(brightness: u8, path_cache: &[PathBuf; LED_BANK_SIZE_FUCKED]) {
    write_paths(
        // TODO replace with array::repeat
        array::from_fn(|_| heapless::String::from_str(&brightness.to_string()).unwrap()),
        path_cache,
    )
    .await;
}

/// Runs a "tick" at which all of the LED settings are
/// asynchronously converted to RGB and sent
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

/// Uses the current mode to calculate the current LED conditions and
/// returns them.
///
/// Returns None of no settings changes are required
fn calculate_settings(mode: &mut WheelMode, last_settings: &Settings) -> Option<Settings> {
    match mode {
        WheelMode::Demo1(demo1_vars) => {
            let mut new_settings = *last_settings;
            // force full saturation/value so the sweeping LED is visible
            // immediately, instead of relying on it having been lit already
            new_settings[demo1_vars.curr_led].saturation = 1f32;
            new_settings[demo1_vars.curr_led].value = 1f32;
            new_settings[demo1_vars.curr_led].hue += RgbHue::from_degrees(1f32);
            if new_settings[demo1_vars.curr_led].hue.into_degrees() > 179f32 {
                new_settings[demo1_vars.curr_led] = Hsv::from_components((0f32, 1f32, 1f32));
                demo1_vars.curr_led += 1;
                if demo1_vars.curr_led >= LED_BANK_SIZE_REAL {
                    demo1_vars.curr_led = 0;
                }
                new_settings[demo1_vars.curr_led] = Hsv::from_components((0f32, 1f32, 1f32));
            }
            Some(new_settings)
        }
        WheelMode::Demo2(demo2_vars) => {
            let mut new_settings = *last_settings;

            match demo2_vars.last_refresh {
                Some(time) => {
                    if time.elapsed() > Duration::from_secs(1) {
                        demo2_vars.last_refresh = Some(tokio::time::Instant::now());
                    } else {
                        return None;
                    }
                }
                None => demo2_vars.last_refresh = Some(tokio::time::Instant::now()),
            }
            match demo2_vars.curr_status {
                Demo2VarsSequence::Red => {
                    new_settings[demo2_vars.curr_led] =
                        Srgb::from_components((1f32, 0f32, 0f32)).into_color();
                    demo2_vars.curr_status = Demo2VarsSequence::Green;
                }
                Demo2VarsSequence::Green => {
                    new_settings[demo2_vars.curr_led] =
                        Srgb::from_components((0f32, 1f32, 0f32)).into_color();
                    demo2_vars.curr_status = Demo2VarsSequence::Blue;
                }
                Demo2VarsSequence::Blue => {
                    new_settings[demo2_vars.curr_led] =
                        Srgb::from_components((0f32, 0f32, 1f32)).into_color();
                    demo2_vars.curr_status = Demo2VarsSequence::Off;
                }
                Demo2VarsSequence::Off => {
                    new_settings[demo2_vars.curr_led] =
                        Srgb::from_components((0f32, 0f32, 0f32)).into_color();
                    demo2_vars.curr_status = Demo2VarsSequence::Red;
                    demo2_vars.curr_led += 1;
                    if demo2_vars.curr_led >= LED_BANK_SIZE_REAL {
                        demo2_vars.curr_led = 0;
                    }
                }
            }

            Some(new_settings)
        }
        WheelMode::Follower(settings) => {
            let mut new_settings = *last_settings;
            // first handle the lr, getting amt_on, the amount of LEDs that should be lit (left to right)
            let amt_on = if settings.lr_val >= settings.lr.max {
                LED_BANK_SIZE_REAL
            } else if settings.lr_val <= settings.lr.min {
                1 // always one led on
            } else {
                // there are effectively 8 steps since the first one is always on
                ((LED_BANK_SIZE_REAL - 1) as f32
                    * ((settings.lr_val - settings.lr.min) / (settings.lr.max - settings.lr.min)))
                    .round() as usize
                    + 1
            };

            // now get the hue of each lit up LED
            let color_range = settings.color.max - settings.color.min;
            let multiplier = if color_range.abs() <= f32::EPSILON {
                0f32
            } else {
                (settings
                    .color_val
                    .clamp(settings.color.min, settings.color.max)
                    - settings.color.min)
                    / color_range
            };
            let hue_raw = (multiplier
                * (settings.color.color_range.1.into_positive_degrees()
                    - settings.color.color_range.0.into_positive_degrees()))
                + settings.color.color_range.0.into_positive_degrees();

            for (index, value) in new_settings.iter_mut().enumerate() {
                if index + 1 > amt_on {
                    *value = Hsv::new_srgb_const(RgbHue::new(0.0f32), 0.0f32, 0.0f32);
                    continue;
                } else {
                    *value = Hsv::new_srgb_const(RgbHue::new(hue_raw), 1.0f32, 1.0f32);
                }
            }

            Some(new_settings)
        }
        WheelMode::BipolarFollower(settings) => {
            let mut new_settings = *last_settings;

            // pick the solid color and full-scale magnitude for whichever
            // side of zero the value currently sits on
            let (hue, magnitude, max) = if settings.val < 0f32 {
                (settings.item.neg_hue, -settings.val, settings.item.neg_max)
            } else {
                (settings.item.pos_hue, settings.val, settings.item.pos_max)
            };

            let amt_on = if max.abs() <= f32::EPSILON {
                1 // always one led on
            } else {
                // there are effectively 8 steps since the first one is always on
                ((LED_BANK_SIZE_REAL - 1) as f32 * (magnitude.clamp(0f32, max) / max)).round()
                    as usize
                    + 1
            };

            for (index, value) in new_settings.iter_mut().enumerate() {
                if index + 1 > amt_on {
                    *value = Hsv::new_srgb_const(RgbHue::new(0.0f32), 0.0f32, 0.0f32);
                } else {
                    *value = Hsv::new_srgb_const(hue, 1.0f32, 1.0f32);
                }
            }

            Some(new_settings)
        }
    }
}

/// Handle recieving a MQTT message, which could either do nothing or
/// mutate the brightness or mode.
///
/// Returns whether existing settings should be reset
fn handle_recv_msg(msg: PlaybackData, brightness: &mut u8, mode: &mut WheelMode) -> bool {
    // handle following
    match mode {
        WheelMode::Demo1(_) => (),
        WheelMode::Demo2(_) => (),
        WheelMode::Follower(settings) => {
            let mut updated = false;
            if msg.topic == settings.lr.topic
                && let Some(val) = msg.values.first()
            {
                settings.lr_val = *val;
                updated = true;
            }
            if msg.topic == settings.color.topic
                && let Some(val) = msg.values.first()
            {
                settings.color_val = *val;
                updated = true;
            }
            if updated {
                return false;
            }
        }
        WheelMode::BipolarFollower(settings) => {
            if msg.topic == settings.item.topic
                && let Some(val) = msg.values.first()
            {
                settings.val = *val;
                return false;
            }
        }
    }
    match msg.topic.as_str() {
        "NERO/Control/LEDBrightness" => {
            let Some(val) = msg.values.first() else {
                warn!("Empty brightness command");
                return false;
            };
            if *val < 0.0f32 || *val > 1.0f32 {
                warn!("Invalid brightness value: {}", val);
                return false;
            }
            *brightness = (val * 255.0f32) as u8;
            false
        }
        "NERO/Control/Mode" => {
            let Some(val) = msg.values.first() else {
                warn!("Empty mode command!");
                return false;
            };
            *mode = WheelMode::from_settings(*val as u8, &msg.values[1..]);
            info!("Switching color controller to mode: {:?}", mode);
            true
        }
        _ => false,
    }
}

pub async fn color_controller(
    cancel_token: CancellationToken,
    mut mqtt_recv_rx: broadcast::Receiver<PlaybackData>,
    default_mode: DefaultWheelMode,
) {
    // cache the paths for quick reuse here, because building a Path is zero cost but also I am afraid
    let path_cache: [PathBuf; LED_BANK_SIZE_FUCKED] = LED_BANK_WRITE_LISTINGS.map(|f| {
        PathBuf::from_str(&format!("{}multi_intensity", f.0)).expect("Could not parse path")
    });
    let brightness_cache: [PathBuf; LED_BANK_SIZE_FUCKED] = LED_BANK_WRITE_LISTINGS
        .map(|f| PathBuf::from_str(&format!("{}brightness", f.0)).expect("Could not parse path"));

    // smaller than fastest human color change (about 13ms)
    let mut tick_size = tokio::time::interval(CALC_CYCLE_TIME);

    // the default boot mode, chosen via CLI/env (falls back to Flappy if unset)
    let mut current_mode = default_mode.into_wheel_mode();
    let mut current_brightness = 175;
    // boot brightness is always zero for some reason with multi-led
    let mut last_brightness = 0;

    // default to all LEDs off before a boot mode kicks in
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

    loop {
        tokio::select! {
            _ = cancel_token.cancelled() => {
                debug!("Shutting down color controller!");
                break;
            },
            _ = tick_size.tick() => {
                // only update brightness if it changes
                if last_brightness != current_brightness {
                    trace!("Updating brightness to {}", current_brightness);
                    execute_brightness_step(current_brightness, &brightness_cache).await;
                    last_brightness = current_brightness;
                }
                // only update settings if they change, as this write can be expensive
                if let Some(settings) = calculate_settings(&mut current_mode, &last_settings) {
                    // yet another check
                    if settings != last_settings {
                        trace!("Writing new settings for color! {:?}", settings);
                        execute_step(settings, &path_cache).await;
                        last_settings = settings;
                    }
                }
            },
            Ok(msg) = mqtt_recv_rx.recv() => {
                if handle_recv_msg(msg, &mut current_brightness, &mut current_mode) {
                    last_settings = [
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
                }
            }
        }
    }
}
