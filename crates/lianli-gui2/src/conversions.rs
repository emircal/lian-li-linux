//! Conversions between lianli-shared Rust types and Slint-generated structs.

use lianli_shared::config::{AppConfig, LcdConfig};
use lianli_shared::fan::{FanConfig, FanCurve, FanSpeed};
use lianli_shared::ipc::{DeviceInfo, TelemetrySnapshot};
use lianli_shared::media::MediaType;
use lianli_shared::rgb::{RgbDeviceCapabilities, RgbMode};
use slint::{ModelRc, SharedString, VecModel};

/// Convert a DeviceInfo + telemetry data into the Slint DeviceData struct.
pub fn device_to_slint(
    device: &DeviceInfo,
    telemetry: &TelemetrySnapshot,
) -> super::DeviceData {
    let fan_rpms = telemetry
        .fan_rpms
        .get(&device.device_id)
        .map(|rpms| {
            rpms.iter()
                .map(|r| r.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();

    let coolant_temp = telemetry
        .coolant_temps
        .get(&device.device_id)
        .map(|t| format!("{t:.1}\u{00B0}C"))
        .unwrap_or_default();

    let resolution = match (device.screen_width, device.screen_height) {
        (Some(w), Some(h)) => format!("{w}x{h}"),
        _ => String::new(),
    };

    let family_name = format!("{:?}", device.family);

    super::DeviceData {
        device_id: SharedString::from(&device.device_id),
        family_name: SharedString::from(&family_name),
        name: SharedString::from(&device.name),
        serial: SharedString::from(device.serial.as_deref().unwrap_or("")),
        has_lcd: device.has_lcd,
        has_fan: device.has_fan,
        has_pump: device.has_pump,
        has_rgb: device.has_rgb,
        fan_rpms: SharedString::from(&fan_rpms),
        coolant_temp: SharedString::from(&coolant_temp),
        resolution: SharedString::from(&resolution),
    }
}

/// Convert a list of DeviceInfo + telemetry into a Slint model.
pub fn devices_to_model(
    devices: &[DeviceInfo],
    telemetry: &TelemetrySnapshot,
) -> ModelRc<super::DeviceData> {
    let items: Vec<super::DeviceData> = devices
        .iter()
        .filter(|d| !matches!(
            d.family,
            lianli_shared::device_id::DeviceFamily::WirelessTx
            | lianli_shared::device_id::DeviceFamily::WirelessRx
            | lianli_shared::device_id::DeviceFamily::DisplaySwitcher
        ))
        .map(|d| device_to_slint(d, telemetry))
        .collect();
    ModelRc::new(VecModel::from(items))
}

// ── LCD conversions ──────────────────────────────────────────────

fn media_type_to_string(mt: &MediaType) -> &'static str {
    match mt {
        MediaType::Image => "Image",
        MediaType::Video => "Video",
        MediaType::Gif => "GIF",
        MediaType::Color => "Solid Color",
        MediaType::Sensor => "Sensor Gauge",
    }
}

pub fn lcd_to_slint(lcd: &LcdConfig) -> super::LcdEntryData {
    let (sensor_label, sensor_unit, sensor_command, sensor_font_path, sensor_decimal_places,
         sensor_update_interval, sensor_value_font_size, sensor_unit_font_size, sensor_label_font_size,
         sensor_start_angle, sensor_sweep_angle, sensor_outer_radius, sensor_thickness,
         sensor_corner_radius, sensor_value_offset, sensor_unit_offset, sensor_label_offset) =
    if let Some(ref s) = lcd.sensor {
        let cmd = match &s.source {
            lianli_shared::media::SensorSourceConfig::Command { cmd } => cmd.clone(),
            lianli_shared::media::SensorSourceConfig::Constant { value } => format!("{value}"),
        };
        (
            s.label.clone(), s.unit.clone(), cmd,
            s.font_path.as_ref().map(|p| p.display().to_string()).unwrap_or_default(),
            s.decimal_places as i32, s.update_interval_ms as i32,
            s.value_font_size as i32, s.unit_font_size as i32, s.label_font_size as i32,
            s.gauge_start_angle as i32, s.gauge_sweep_angle as i32,
            s.gauge_outer_radius as i32, s.gauge_thickness as i32,
            s.bar_corner_radius as i32,
            s.value_offset, s.unit_offset, s.label_offset,
        )
    } else {
        (String::new(), String::new(), String::new(), String::new(),
         0, 1000, 120, 40, 30, 135, 270, 200, 30, 5, 0, 0, 0)
    };

    let [r, g, b] = lcd.rgb.unwrap_or([0, 0, 0]);

    super::LcdEntryData {
        serial: SharedString::from(lcd.serial.as_deref().unwrap_or("")),
        media_type: SharedString::from(media_type_to_string(&lcd.media_type)),
        path: SharedString::from(lcd.path.as_ref().map(|p| p.display().to_string()).unwrap_or_default()),
        fps: lcd.fps.map(|f| f as i32).unwrap_or(30),
        orientation: lcd.orientation as i32,
        rgb_r: r as i32,
        rgb_g: g as i32,
        rgb_b: b as i32,
        sensor_label: SharedString::from(&sensor_label),
        sensor_unit: SharedString::from(&sensor_unit),
        sensor_command: SharedString::from(&sensor_command),
        sensor_font_path: SharedString::from(&sensor_font_path),
        sensor_decimal_places: sensor_decimal_places,
        sensor_update_interval: sensor_update_interval,
        sensor_value_font_size: sensor_value_font_size,
        sensor_unit_font_size: sensor_unit_font_size,
        sensor_label_font_size: sensor_label_font_size,
        sensor_start_angle: sensor_start_angle,
        sensor_sweep_angle: sensor_sweep_angle,
        sensor_outer_radius: sensor_outer_radius,
        sensor_thickness: sensor_thickness,
        sensor_corner_radius: sensor_corner_radius,
        sensor_value_offset: sensor_value_offset,
        sensor_unit_offset: sensor_unit_offset,
        sensor_label_offset: sensor_label_offset,
    }
}

pub fn lcd_entries_to_model(lcds: &[LcdConfig]) -> ModelRc<super::LcdEntryData> {
    let items: Vec<_> = lcds.iter().map(lcd_to_slint).collect();
    ModelRc::new(VecModel::from(items))
}

/// Build device option strings for LCD device selector (e.g. "SL-INF (serial)")
pub fn lcd_device_options(devices: &[DeviceInfo]) -> ModelRc<SharedString> {
    let items: Vec<SharedString> = devices
        .iter()
        .filter(|d| d.has_lcd)
        .map(|d| {
            let serial = d.serial.as_deref().unwrap_or(&d.device_id);
            SharedString::from(serial)
        })
        .collect();
    ModelRc::new(VecModel::from(items))
}

// ── Fan conversions ──────────────────────────────────────────────

// Path commands use normalized [0,1] coordinates within the plot area.
// Slint's Path interprets coordinates relative to the element's width/height.
const TEMP_MIN: f32 = 20.0;
const TEMP_MAX: f32 = 100.0;
const SPEED_MIN: f32 = 0.0;
const SPEED_MAX: f32 = 100.0;

/// Normalized x [0,1] from temperature.
fn temp_to_norm(temp: f32) -> f32 {
    (temp - TEMP_MIN) / (TEMP_MAX - TEMP_MIN)
}

/// Normalized y [0,1] from speed (inverted: 100% = top = 0).
fn speed_to_norm(speed: f32) -> f32 {
    1.0 - (speed - SPEED_MIN) / (SPEED_MAX - SPEED_MIN)
}

/// Build an SVG path string for the curve polyline.
fn build_curve_path(sorted: &[(f32, f32)]) -> String {
    if sorted.is_empty() {
        return String::new();
    }
    let mut path = String::new();
    for (i, &(temp, speed)) in sorted.iter().enumerate() {
        let x = temp_to_norm(temp);
        let y = speed_to_norm(speed);
        if i == 0 {
            path.push_str(&format!("M {x} {y}"));
        } else {
            path.push_str(&format!(" L {x} {y}"));
        }
    }
    path
}

/// Build clamp line path extending from first point left to axis edge.
fn build_clamp_left(sorted: &[(f32, f32)]) -> String {
    if sorted.is_empty() {
        return String::new();
    }
    let first = sorted[0];
    if first.0 <= TEMP_MIN {
        return String::new();
    }
    let y = speed_to_norm(first.1);
    let x_from = 0.0;
    let x_to = temp_to_norm(first.0);
    // Dashed line approximation: series of short segments
    let _total = x_to - x_from;
    let dash = 0.015_f32;
    let gap = 0.01_f32;
    let mut path = String::new();
    let mut x = x_from;
    let mut drawing = true;
    while x < x_to {
        let seg_end = (x + if drawing { dash } else { gap }).min(x_to);
        if drawing {
            if path.is_empty() {
                path.push_str(&format!("M {x} {y}"));
            } else {
                path.push_str(&format!(" M {x} {y}"));
            }
            path.push_str(&format!(" L {seg_end} {y}"));
        }
        x = seg_end;
        drawing = !drawing;
    }
    path
}

/// Build clamp line path extending from last point right to axis edge.
fn build_clamp_right(sorted: &[(f32, f32)]) -> String {
    if sorted.is_empty() {
        return String::new();
    }
    let last = sorted[sorted.len() - 1];
    if last.0 >= TEMP_MAX {
        return String::new();
    }
    let y = speed_to_norm(last.1);
    let x_from = temp_to_norm(last.0);
    let x_to = 1.0;
    let dash = 0.015_f32;
    let gap = 0.01_f32;
    let mut path = String::new();
    let mut x = x_from;
    let mut drawing = true;
    while x < x_to {
        let seg_end = (x + if drawing { dash } else { gap }).min(x_to);
        if drawing {
            if path.is_empty() {
                path.push_str(&format!("M {x} {y}"));
            } else {
                path.push_str(&format!(" M {x} {y}"));
            }
            path.push_str(&format!(" L {seg_end} {y}"));
        }
        x = seg_end;
        drawing = !drawing;
    }
    path
}

pub fn fan_curve_to_slint(curve: &FanCurve, plot_width: f32, plot_height: f32) -> super::FanCurveData {
    let mut sorted_points: Vec<(f32, f32)> = curve.curve.clone();
    sorted_points.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    let points: Vec<super::CurvePoint> = sorted_points
        .iter()
        .map(|&(temp, speed)| super::CurvePoint {
            temp,
            speed,
            px: temp_to_norm(temp) * plot_width,
            py: speed_to_norm(speed) * plot_height,
        })
        .collect();

    super::FanCurveData {
        name: SharedString::from(&curve.name),
        temp_command: SharedString::from(&curve.temp_command),
        points: ModelRc::new(VecModel::from(points)),
        curve_path: SharedString::from(build_curve_path(&sorted_points)),
        clamp_left_path: SharedString::from(build_clamp_left(&sorted_points)),
        clamp_right_path: SharedString::from(build_clamp_right(&sorted_points)),
    }
}

pub fn fan_curves_to_model(curves: &[FanCurve], plot_width: f32, plot_height: f32) -> ModelRc<super::FanCurveData> {
    let items: Vec<_> = curves.iter().map(|c| fan_curve_to_slint(c, plot_width, plot_height)).collect();
    ModelRc::new(VecModel::from(items))
}

pub fn curve_names_to_model(curves: &[FanCurve]) -> ModelRc<SharedString> {
    let items: Vec<SharedString> = curves.iter().map(|c| SharedString::from(&c.name)).collect();
    ModelRc::new(VecModel::from(items))
}

/// Build the speed options dropdown list: ["Off", curve1, curve2, ..., "Constant PWM", "MB Sync"]
pub fn speed_options_model(curves: &[FanCurve], _has_mb_sync: bool) -> ModelRc<SharedString> {
    let mut items = vec![SharedString::from("Off")];
    for c in curves {
        items.push(SharedString::from(&c.name));
    }
    items.push(SharedString::from("Constant PWM"));
    items.push(SharedString::from("MB Sync"));
    ModelRc::new(VecModel::from(items))
}

pub fn fan_groups_to_model(
    fan_config: &FanConfig,
    devices: &[DeviceInfo],
) -> ModelRc<super::FanGroupData> {
    let items: Vec<super::FanGroupData> = fan_config
        .speeds
        .iter()
        .map(|group| {
            let device_id = group.device_id.clone().unwrap_or_default();
            let dev = devices.iter().find(|d| d.device_id == device_id);
            let device_name = dev.map(|d| d.name.clone()).unwrap_or_default();
            let fan_count = dev.and_then(|d| d.fan_count).unwrap_or(4) as i32;
            let per_fan_control = dev.and_then(|d| d.per_fan_control).unwrap_or(false);
            let mb_sync_support = dev.map(|d| d.mb_sync_support).unwrap_or(false);

            let slots: Vec<super::FanSpeedSlot> = group.speeds.iter().map(|s| {
                match s {
                    FanSpeed::Constant(0) => super::FanSpeedSlot {
                        dropdown_value: SharedString::from("Off"),
                        pwm_percent: 0,
                        display_mode: SharedString::from("off"),
                    },
                    FanSpeed::Constant(pwm) => super::FanSpeedSlot {
                        dropdown_value: SharedString::from("Constant PWM"),
                        pwm_percent: ((*pwm as f32 / 255.0) * 100.0).round() as i32,
                        display_mode: SharedString::from("constant"),
                    },
                    FanSpeed::Curve(name) if name == "__mb_sync__" => super::FanSpeedSlot {
                        dropdown_value: SharedString::from("MB Sync"),
                        pwm_percent: 0,
                        display_mode: SharedString::from("mb_sync"),
                    },
                    FanSpeed::Curve(name) => super::FanSpeedSlot {
                        dropdown_value: SharedString::from(name.as_str()),
                        pwm_percent: 0,
                        display_mode: SharedString::from("curve"),
                    },
                }
            }).collect();

            super::FanGroupData {
                device_id: SharedString::from(&device_id),
                device_name: SharedString::from(&device_name),
                fan_count,
                per_fan_control: per_fan_control,
                mb_sync_support: mb_sync_support,
                slots: ModelRc::new(VecModel::from(slots)),
            }
        })
        .collect();
    ModelRc::new(VecModel::from(items))
}

// ── RGB conversions ──────────────────────────────────────────────

fn rgb_mode_to_string(mode: &RgbMode) -> String {
    format!("{mode:?}")
}

pub fn rgb_devices_to_model(
    capabilities: &[RgbDeviceCapabilities],
    config: &AppConfig,
) -> ModelRc<super::RgbDeviceData> {
    let rgb_config = config.rgb.as_ref();
    let device_configs = rgb_config.map(|r| &r.devices);

    let items: Vec<super::RgbDeviceData> = capabilities
        .iter()
        .map(|cap| {
            let dev_cfg = device_configs
                .and_then(|devs| devs.iter().find(|d| d.device_id == cap.device_id));

            let mb_rgb_sync = dev_cfg.map(|d| d.mb_rgb_sync).unwrap_or(false);

            let zones: Vec<super::RgbZoneData> = cap
                .zones
                .iter()
                .enumerate()
                .map(|(zidx, zone_info)| {
                    let zone_cfg = dev_cfg.and_then(|d| {
                        d.zones.iter().find(|z| z.zone_index == zidx as u8)
                    });

                    let (mode, colors, speed, brightness, direction, scope, swap_lr, swap_tb) =
                        if let Some(zcfg) = zone_cfg {
                            let e = &zcfg.effect;
                            let colors: Vec<super::RgbColorData> = e.colors
                                .iter()
                                .map(|c| super::RgbColorData { r: c[0] as i32, g: c[1] as i32, b: c[2] as i32 })
                                .collect();
                            (
                                rgb_mode_to_string(&e.mode),
                                colors,
                                e.speed as i32,
                                e.brightness as i32,
                                format!("{:?}", e.direction),
                                format!("{:?}", e.scope),
                                zcfg.swap_lr,
                                zcfg.swap_tb,
                            )
                        } else {
                            (
                                "Off".to_string(),
                                vec![super::RgbColorData { r: 255, g: 0, b: 0 }],
                                2, 3,
                                "Clockwise".to_string(),
                                "All".to_string(),
                                false, false,
                            )
                        };

                    super::RgbZoneData {
                        zone_index: zidx as i32,
                        zone_name: SharedString::from(&zone_info.name),
                        led_count: zone_info.led_count as i32,
                        mode: SharedString::from(&mode),
                        colors: ModelRc::new(VecModel::from(colors)),
                        speed,
                        brightness,
                        direction: SharedString::from(&direction),
                        scope: SharedString::from(&scope),
                        swap_lr,
                        swap_tb,
                    }
                })
                .collect();

            let supported_modes: Vec<SharedString> = cap
                .supported_modes
                .iter()
                .map(|m| SharedString::from(rgb_mode_to_string(m)))
                .collect();

            // Flatten all scopes across zones into a unique set
            let mut all_scopes: Vec<String> = cap
                .supported_scopes
                .iter()
                .flat_map(|s| s.iter().map(|sc| format!("{sc:?}")))
                .collect();
            all_scopes.sort();
            all_scopes.dedup();
            let supported_scopes: Vec<SharedString> = all_scopes
                .iter()
                .map(|s| SharedString::from(s.as_str()))
                .collect();

            super::RgbDeviceData {
                device_id: SharedString::from(&cap.device_id),
                device_name: SharedString::from(&cap.device_name),
                total_leds: cap.total_led_count as i32,
                mb_rgb_sync: mb_rgb_sync,
                supports_mb_sync: cap.supports_mb_rgb_sync,
                supports_direction: cap.supports_direction,
                supported_modes: ModelRc::new(VecModel::from(supported_modes)),
                supported_scopes: ModelRc::new(VecModel::from(supported_scopes)),
                zones: ModelRc::new(VecModel::from(zones)),
            }
        })
        .collect();
    ModelRc::new(VecModel::from(items))
}
