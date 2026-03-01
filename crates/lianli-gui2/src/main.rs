mod backend;
mod conversions;
mod ipc_client;
mod state;

use lianli_shared::config::AppConfig;
use lianli_shared::fan::{FanConfig, FanCurve, FanGroup, FanSpeed};
use lianli_shared::ipc::IpcRequest;
use lianli_shared::rgb::{
    RgbAppConfig, RgbDeviceConfig, RgbDirection, RgbEffect, RgbMode, RgbScope, RgbZoneConfig,
};
use std::sync::{Arc, Mutex};

slint::include_modules!();

/// Shared mutable config that callbacks can read/write.
/// The backend thread loads it; callbacks mutate it; save sends it.
type SharedConfig = Arc<Mutex<Option<AppConfig>>>;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("lianli_gui2=info".parse().unwrap()),
        )
        .init();

    let window = MainWindow::new().expect("Failed to create main window");
    let backend = backend::start(window.as_weak());

    // Shared config state
    let config: SharedConfig = Arc::new(Mutex::new(None));

    // ── Refresh devices ──
    {
        let tx = backend.tx.clone();
        window.on_refresh_devices(move || {
            let _ = tx.send(backend::BackendCommand::RefreshDevices);
        });
    }

    // ── Save config ──
    {
        let tx = backend.tx.clone();
        let cfg = config.clone();
        window.on_save_config(move || {
            let guard = cfg.lock().unwrap();
            if let Some(c) = guard.clone() {
                let _ = tx.send(backend::BackendCommand::SaveConfig(c));
            }
        });
    }

    // ── Store config when backend loads it ──
    // We hook into the backend's config loading by having the backend
    // also push config to our shared state.
    // Actually, we'll set config from a polling mechanism. For now,
    // let's load it on startup and update on save.
    {
        let cfg = config.clone();
        std::thread::spawn(move || {
            // Wait a moment for backend to connect
            std::thread::sleep(std::time::Duration::from_millis(500));
            let loaded: Option<AppConfig> =
                ipc_client::send_request(&IpcRequest::GetConfig)
                    .and_then(ipc_client::unwrap_response)
                    .ok();
            if let Some(c) = loaded {
                *cfg.lock().unwrap() = Some(c);
            }
        });
    }

    // ── Toggle OpenRGB ──
    {
        let tx = backend.tx.clone();
        let cfg = config.clone();
        window.on_toggle_openrgb(move |enabled| {
            let mut guard = cfg.lock().unwrap();
            if let Some(ref mut c) = *guard {
                let rgb = c.rgb.get_or_insert_with(Default::default);
                rgb.openrgb_server = enabled;
                let _ = tx.send(backend::BackendCommand::SaveConfig(c.clone()));
            }
        });
    }

    // ── RGB callbacks ──
    wire_rgb_callbacks(&window, &backend, &config);

    // ── Fan callbacks ──
    wire_fan_callbacks(&window, &backend, &config);

    // ── LCD callbacks ──
    wire_lcd_callbacks(&window, &backend, &config);

    window.run().expect("Failed to run Slint event loop");
    backend.send(backend::BackendCommand::Shutdown);
}

fn wire_rgb_callbacks(
    window: &MainWindow,
    backend: &backend::BackendHandle,
    config: &SharedConfig,
) {
    // RGB set mode — sends SetRgbEffect to daemon + updates config
    {
        let tx = backend.tx.clone();
        let cfg = config.clone();
        window.on_rgb_set_mode(move |dev_id, zone, mode| {
            let dev_id = dev_id.to_string();
            let zone = zone as u8;
            let mode_enum = parse_rgb_mode(&mode);

            let effect = with_zone_effect(&cfg, &dev_id, zone, |e| {
                e.mode = mode_enum;
            });

            let _ = tx.send(backend::BackendCommand::IpcRequest(
                IpcRequest::SetRgbEffect {
                    device_id: dev_id,
                    zone,
                    effect,
                },
            ));
        });
    }

    {
        let tx = backend.tx.clone();
        let cfg = config.clone();
        window.on_rgb_set_speed(move |dev_id, zone, speed| {
            let dev_id = dev_id.to_string();
            let zone = zone as u8;
            let effect = with_zone_effect(&cfg, &dev_id, zone, |e| {
                e.speed = speed as u8;
            });
            let _ = tx.send(backend::BackendCommand::IpcRequest(
                IpcRequest::SetRgbEffect {
                    device_id: dev_id,
                    zone,
                    effect,
                },
            ));
        });
    }

    {
        let tx = backend.tx.clone();
        let cfg = config.clone();
        window.on_rgb_set_brightness(move |dev_id, zone, brightness| {
            let dev_id = dev_id.to_string();
            let zone = zone as u8;
            let effect = with_zone_effect(&cfg, &dev_id, zone, |e| {
                e.brightness = brightness as u8;
            });
            let _ = tx.send(backend::BackendCommand::IpcRequest(
                IpcRequest::SetRgbEffect {
                    device_id: dev_id,
                    zone,
                    effect,
                },
            ));
        });
    }

    {
        let tx = backend.tx.clone();
        let cfg = config.clone();
        window.on_rgb_set_direction(move |dev_id, zone, dir| {
            let dev_id = dev_id.to_string();
            let zone = zone as u8;
            let effect = with_zone_effect(&cfg, &dev_id, zone, |e| {
                e.direction = parse_rgb_direction(&dir);
            });
            let _ = tx.send(backend::BackendCommand::IpcRequest(
                IpcRequest::SetRgbEffect {
                    device_id: dev_id,
                    zone,
                    effect,
                },
            ));
        });
    }

    {
        let tx = backend.tx.clone();
        let cfg = config.clone();
        window.on_rgb_set_scope(move |dev_id, zone, scope| {
            let dev_id = dev_id.to_string();
            let zone = zone as u8;
            let effect = with_zone_effect(&cfg, &dev_id, zone, |e| {
                e.scope = parse_rgb_scope(&scope);
            });
            let _ = tx.send(backend::BackendCommand::IpcRequest(
                IpcRequest::SetRgbEffect {
                    device_id: dev_id,
                    zone,
                    effect,
                },
            ));
        });
    }

    {
        let tx = backend.tx.clone();
        let cfg = config.clone();
        window.on_rgb_set_color(move |dev_id, zone, cidx, r, g, b| {
            let dev_id = dev_id.to_string();
            let zone = zone as u8;
            let effect = with_zone_effect(&cfg, &dev_id, zone, |e| {
                let cidx = cidx as usize;
                while e.colors.len() <= cidx {
                    e.colors.push([255, 255, 255]);
                }
                e.colors[cidx] = [r as u8, g as u8, b as u8];
            });
            let _ = tx.send(backend::BackendCommand::IpcRequest(
                IpcRequest::SetRgbEffect {
                    device_id: dev_id,
                    zone,
                    effect,
                },
            ));
        });
    }

    {
        let tx = backend.tx.clone();
        let cfg = config.clone();
        window.on_rgb_toggle_mb_sync(move |dev_id, enabled| {
            let dev_id = dev_id.to_string();
            // Update config
            {
                let mut guard = cfg.lock().unwrap();
                if let Some(ref mut c) = *guard {
                    let rgb = c.rgb.get_or_insert_with(Default::default);
                    // MB sync is controller-wide: update all sibling port devices
                    let base_id = dev_id.split(":port").next().unwrap_or(&dev_id);
                    for dev_cfg in &mut rgb.devices {
                        if dev_cfg.device_id.starts_with(base_id) {
                            dev_cfg.mb_rgb_sync = enabled;
                        }
                    }
                    // If device not in config yet, add it
                    if !rgb.devices.iter().any(|d| d.device_id == dev_id) {
                        rgb.devices.push(RgbDeviceConfig {
                            device_id: dev_id.clone(),
                            mb_rgb_sync: enabled,
                            zones: vec![],
                        });
                    }
                }
            }
            let _ = tx.send(backend::BackendCommand::IpcRequest(
                IpcRequest::SetMbRgbSync {
                    device_id: dev_id,
                    enabled,
                },
            ));
        });
    }

    {
        let tx = backend.tx.clone();
        let cfg = config.clone();
        window.on_rgb_apply_to_all(move |dev_id| {
            let dev_id = dev_id.to_string();
            let guard = cfg.lock().unwrap();
            if let Some(ref c) = *guard {
                if let Some(rgb) = &c.rgb {
                    if let Some(dev_cfg) = rgb.devices.iter().find(|d| d.device_id == dev_id) {
                        // Apply zone 0's effect to all zones
                        if let Some(z0) = dev_cfg.zones.first() {
                            let effect = z0.effect.clone();
                            for zone_cfg in &dev_cfg.zones {
                                let _ = tx.send(backend::BackendCommand::IpcRequest(
                                    IpcRequest::SetRgbEffect {
                                        device_id: dev_id.clone(),
                                        zone: zone_cfg.zone_index,
                                        effect: effect.clone(),
                                    },
                                ));
                            }
                        }
                    }
                }
            }
        });
    }

    {
        let tx = backend.tx.clone();
        let cfg = config.clone();
        window.on_rgb_toggle_swap_lr(move |dev_id, zone| {
            let dev_id = dev_id.to_string();
            let zone = zone as u8;
            let (swap_lr, swap_tb) = {
                let mut guard = cfg.lock().unwrap();
                if let Some(ref mut c) = *guard {
                    let rgb = c.rgb.get_or_insert_with(Default::default);
                    let dev = get_or_create_device_config(rgb, &dev_id);
                    let zcfg = get_or_create_zone_config(dev, zone);
                    zcfg.swap_lr = !zcfg.swap_lr;
                    (zcfg.swap_lr, zcfg.swap_tb)
                } else {
                    return;
                }
            };
            let _ = tx.send(backend::BackendCommand::IpcRequest(
                IpcRequest::SetFanDirection {
                    device_id: dev_id,
                    zone,
                    swap_lr,
                    swap_tb,
                },
            ));
        });
    }

    {
        let tx = backend.tx.clone();
        let cfg = config.clone();
        window.on_rgb_toggle_swap_tb(move |dev_id, zone| {
            let dev_id = dev_id.to_string();
            let zone = zone as u8;
            let (swap_lr, swap_tb) = {
                let mut guard = cfg.lock().unwrap();
                if let Some(ref mut c) = *guard {
                    let rgb = c.rgb.get_or_insert_with(Default::default);
                    let dev = get_or_create_device_config(rgb, &dev_id);
                    let zcfg = get_or_create_zone_config(dev, zone);
                    zcfg.swap_tb = !zcfg.swap_tb;
                    (zcfg.swap_lr, zcfg.swap_tb)
                } else {
                    return;
                }
            };
            let _ = tx.send(backend::BackendCommand::IpcRequest(
                IpcRequest::SetFanDirection {
                    device_id: dev_id,
                    zone,
                    swap_lr,
                    swap_tb,
                },
            ));
        });
    }
}

fn wire_fan_callbacks(
    window: &MainWindow,
    _backend: &backend::BackendHandle,
    config: &SharedConfig,
) {
    {
        let cfg = config.clone();
        let weak = window.as_weak();
        window.on_fan_add_curve(move || {
            let mut guard = cfg.lock().unwrap();
            if let Some(ref mut c) = *guard {
                let n = c.fan_curves.len() + 1;
                c.fan_curves.push(FanCurve {
                    name: format!("curve-{n}"),
                    temp_command: "cat /sys/class/thermal/thermal_zone0/temp | awk '{print $1/1000}'"
                        .to_string(),
                    curve: vec![(30.0, 30.0), (50.0, 50.0), (70.0, 80.0), (85.0, 100.0)],
                });
                refresh_fan_ui(&weak, c);
            }
        });
    }

    {
        let cfg = config.clone();
        let weak = window.as_weak();
        window.on_fan_remove_curve(move |idx| {
            let mut guard = cfg.lock().unwrap();
            if let Some(ref mut c) = *guard {
                let idx = idx as usize;
                if idx < c.fan_curves.len() {
                    c.fan_curves.remove(idx);
                    refresh_fan_ui(&weak, c);
                }
            }
        });
    }

    {
        let cfg = config.clone();
        let weak = window.as_weak();
        window.on_fan_rename_curve(move |idx, name| {
            let mut guard = cfg.lock().unwrap();
            if let Some(ref mut c) = *guard {
                if let Some(curve) = c.fan_curves.get_mut(idx as usize) {
                    curve.name = name.to_string();
                    refresh_fan_ui(&weak, c);
                }
            }
        });
    }

    {
        let cfg = config.clone();
        window.on_fan_set_temp_command(move |idx, cmd| {
            let mut guard = cfg.lock().unwrap();
            if let Some(ref mut c) = *guard {
                if let Some(curve) = c.fan_curves.get_mut(idx as usize) {
                    curve.temp_command = cmd.to_string();
                }
            }
        });
    }

    {
        let cfg = config.clone();
        let weak = window.as_weak();
        window.on_fan_point_moved(move |cidx, pidx, temp, speed| {
            let mut guard = cfg.lock().unwrap();
            if let Some(ref mut c) = *guard {
                if let Some(curve) = c.fan_curves.get_mut(cidx as usize) {
                    if let Some(pt) = curve.curve.get_mut(pidx as usize) {
                        pt.0 = temp.round().clamp(20.0, 100.0);
                        pt.1 = speed.round().clamp(0.0, 100.0);
                        refresh_fan_ui(&weak, c);
                    }
                }
            }
        });
    }

    {
        let cfg = config.clone();
        let weak = window.as_weak();
        window.on_fan_point_added(move |cidx, temp, speed| {
            let mut guard = cfg.lock().unwrap();
            if let Some(ref mut c) = *guard {
                if let Some(curve) = c.fan_curves.get_mut(cidx as usize) {
                    curve.curve.push((
                        temp.round().clamp(20.0, 100.0),
                        speed.round().clamp(0.0, 100.0),
                    ));
                    refresh_fan_ui(&weak, c);
                }
            }
        });
    }

    {
        let cfg = config.clone();
        let weak = window.as_weak();
        window.on_fan_point_removed(move |cidx, pidx| {
            let mut guard = cfg.lock().unwrap();
            if let Some(ref mut c) = *guard {
                if let Some(curve) = c.fan_curves.get_mut(cidx as usize) {
                    let pidx = pidx as usize;
                    if pidx < curve.curve.len() {
                        curve.curve.remove(pidx);
                        refresh_fan_ui(&weak, c);
                    }
                }
            }
        });
    }

    // Fan speed assignment
    {
        let cfg = config.clone();
        let weak = window.as_weak();
        window.on_fan_set_slot_speed(move |dev_id, slot, val| {
            let dev_id = dev_id.to_string();
            let slot = slot as usize;
            let val = val.to_string();
            let mut guard = cfg.lock().unwrap();
            if let Some(ref mut c) = *guard {
                let fc = c.fans.get_or_insert_with(|| FanConfig {
                    speeds: vec![],
                    update_interval_ms: 1000,
                });
                let group = fc.speeds.iter_mut().find(|g| g.device_id.as_deref() == Some(&dev_id));
                let group = if let Some(g) = group {
                    g
                } else {
                    fc.speeds.push(FanGroup {
                        device_id: Some(dev_id.clone()),
                        speeds: [FanSpeed::Constant(0), FanSpeed::Constant(0), FanSpeed::Constant(0), FanSpeed::Constant(0)],
                    });
                    fc.speeds.last_mut().unwrap()
                };

                let speed: FanSpeed = match val.as_str() {
                    "Off" => FanSpeed::Constant(0),
                    "Constant PWM" => FanSpeed::Constant(128),
                    "MB Sync" => FanSpeed::Curve("__mb_sync__".to_string()),
                    curve_name => FanSpeed::Curve(curve_name.to_string()),
                };
                if slot < 4 {
                    group.speeds[slot] = speed;
                }
                refresh_fan_ui(&weak, c);
            }
        });
    }

    {
        let cfg = config.clone();
        let weak = window.as_weak();
        window.on_fan_set_slot_pwm(move |dev_id, slot, percent| {
            let dev_id = dev_id.to_string();
            let slot = slot as usize;
            let mut guard = cfg.lock().unwrap();
            if let Some(ref mut c) = *guard {
                if let Some(fc) = &mut c.fans {
                    if let Some(group) = fc.speeds.iter_mut().find(|g| g.device_id.as_deref() == Some(&dev_id)) {
                        if slot < 4 {
                            group.speeds[slot] = FanSpeed::Constant(((percent as f32 / 100.0) * 255.0).round() as u8);
                            refresh_fan_ui(&weak, c);
                        }
                    }
                }
            }
        });
    }
}

fn wire_lcd_callbacks(
    window: &MainWindow,
    _backend: &backend::BackendHandle,
    config: &SharedConfig,
) {
    {
        let cfg = config.clone();
        let weak = window.as_weak();
        window.on_add_lcd(move || {
            let mut guard = cfg.lock().unwrap();
            if let Some(ref mut c) = *guard {
                c.lcds.push(lianli_shared::config::LcdConfig {
                    index: None,
                    serial: None,
                    media_type: lianli_shared::media::MediaType::Image,
                    path: None,
                    fps: Some(30.0),
                    rgb: None,
                    orientation: 0.0,
                    sensor: None,
                });
                refresh_lcd_ui(&weak, c);
            }
        });
    }

    {
        let cfg = config.clone();
        let weak = window.as_weak();
        window.on_remove_lcd(move |idx| {
            let mut guard = cfg.lock().unwrap();
            if let Some(ref mut c) = *guard {
                let idx = idx as usize;
                if idx < c.lcds.len() {
                    c.lcds.remove(idx);
                    refresh_lcd_ui(&weak, c);
                }
            }
        });
    }

    {
        let cfg = config.clone();
        let weak = window.as_weak();
        window.on_update_lcd_field(move |idx, field, val| {
            let mut guard = cfg.lock().unwrap();
            if let Some(ref mut c) = *guard {
                let idx = idx as usize;
                if let Some(lcd) = c.lcds.get_mut(idx) {
                    let field = field.to_string();
                    let val = val.to_string();
                    match field.as_str() {
                        "serial" => lcd.serial = Some(val),
                        "media_type" => {
                            lcd.media_type = match val.as_str() {
                                "Image" => lianli_shared::media::MediaType::Image,
                                "Video" => lianli_shared::media::MediaType::Video,
                                "GIF" => lianli_shared::media::MediaType::Gif,
                                "Solid Color" => lianli_shared::media::MediaType::Color,
                                "Sensor Gauge" => lianli_shared::media::MediaType::Sensor,
                                _ => lcd.media_type,
                            };
                        }
                        "path" => lcd.path = Some(std::path::PathBuf::from(val)),
                        "orientation" => lcd.orientation = val.parse().unwrap_or(0.0),
                        "sensor_label" => {
                            lcd.sensor.get_or_insert_with(default_sensor).label = val;
                        }
                        "sensor_unit" => {
                            lcd.sensor.get_or_insert_with(default_sensor).unit = val;
                        }
                        "sensor_command" => {
                            lcd.sensor.get_or_insert_with(default_sensor).source =
                                lianli_shared::media::SensorSourceConfig::Command { cmd: val };
                        }
                        "sensor_font_path" => {
                            lcd.sensor.get_or_insert_with(default_sensor).font_path =
                                Some(std::path::PathBuf::from(val));
                        }
                        _ => {}
                    }
                    refresh_lcd_ui(&weak, c);
                }
            }
        });
    }

    {
        let cfg = config.clone();
        let weak = window.as_weak();
        window.on_pick_lcd_file(move |idx| {
            let cfg2 = cfg.clone();
            let weak2 = weak.clone();
            let idx = idx as usize;
            // Run file picker on a thread (rfd blocks)
            std::thread::spawn(move || {
                let file = rfd::FileDialog::new()
                    .add_filter(
                        "Media",
                        &["jpg", "jpeg", "png", "bmp", "gif", "mp4", "avi", "mkv", "webm"],
                    )
                    .pick_file();
                if let Some(path) = file {
                    let mut guard = cfg2.lock().unwrap();
                    if let Some(ref mut c) = *guard {
                        if let Some(lcd) = c.lcds.get_mut(idx) {
                            lcd.path = Some(path);
                            refresh_lcd_ui(&weak2, c);
                        }
                    }
                }
            });
        });
    }
}

// ── Helpers ──

fn refresh_fan_ui(weak: &slint::Weak<MainWindow>, config: &AppConfig) {
    let plot_w = 400.0;
    let plot_h = 160.0;
    let curves = config.fan_curves.clone();
    let fans = config.fans.clone();
    let devices: Vec<lianli_shared::ipc::DeviceInfo> =
        ipc_client::send_request(&IpcRequest::ListDevices)
            .and_then(ipc_client::unwrap_response)
            .unwrap_or_default();

    let weak = weak.clone();
    slint::invoke_from_event_loop(move || {
        if let Some(w) = weak.upgrade() {
            w.set_fan_curves(conversions::fan_curves_to_model(&curves, plot_w, plot_h));
            w.set_curve_names(conversions::curve_names_to_model(&curves));
            w.set_fan_speed_options(conversions::speed_options_model(&curves, true));
            w.set_config_dirty(true);
            if let Some(ref fc) = fans {
                w.set_fan_groups(conversions::fan_groups_to_model(fc, &devices));
            }
        }
    })
    .ok();
}

fn refresh_lcd_ui(weak: &slint::Weak<MainWindow>, config: &AppConfig) {
    let lcds = config.lcds.clone();
    let weak = weak.clone();
    slint::invoke_from_event_loop(move || {
        if let Some(w) = weak.upgrade() {
            w.set_lcd_entries(conversions::lcd_entries_to_model(&lcds));
            w.set_config_dirty(true);
        }
    })
    .ok();
}

fn default_sensor() -> lianli_shared::media::SensorDescriptor {
    lianli_shared::media::SensorDescriptor {
        label: "CPU".to_string(),
        unit: "°C".to_string(),
        source: lianli_shared::media::SensorSourceConfig::Command {
            cmd: String::new(),
        },
        text_color: [255, 255, 255],
        background_color: [0, 0, 0],
        gauge_background_color: [40, 40, 40],
        gauge_ranges: vec![],
        update_interval_ms: 1000,
        gauge_start_angle: 135.0,
        gauge_sweep_angle: 270.0,
        gauge_outer_radius: 200.0,
        gauge_thickness: 30.0,
        bar_corner_radius: 5.0,
        value_font_size: 120.0,
        unit_font_size: 40.0,
        label_font_size: 30.0,
        font_path: None,
        decimal_places: 0,
        value_offset: 0,
        unit_offset: 0,
        label_offset: 0,
    }
}

/// Get or update an RGB zone's effect in the shared config, returning the updated effect.
fn with_zone_effect(
    cfg: &SharedConfig,
    dev_id: &str,
    zone: u8,
    mutate: impl FnOnce(&mut RgbEffect),
) -> RgbEffect {
    let mut guard = cfg.lock().unwrap();
    let c = match guard.as_mut() {
        Some(c) => c,
        None => {
            let mut e = RgbEffect {
                mode: RgbMode::Static,
                colors: vec![[255, 255, 255]],
                speed: 2,
                brightness: 4,
                direction: RgbDirection::Clockwise,
                scope: RgbScope::All,
            };
            mutate(&mut e);
            return e;
        }
    };

    let rgb = c.rgb.get_or_insert_with(Default::default);
    let dev = get_or_create_device_config(rgb, dev_id);
    let zcfg = get_or_create_zone_config(dev, zone);
    mutate(&mut zcfg.effect);
    zcfg.effect.clone()
}

fn get_or_create_device_config<'a>(
    rgb: &'a mut RgbAppConfig,
    dev_id: &str,
) -> &'a mut RgbDeviceConfig {
    if !rgb.devices.iter().any(|d| d.device_id == dev_id) {
        rgb.devices.push(RgbDeviceConfig {
            device_id: dev_id.to_string(),
            mb_rgb_sync: false,
            zones: vec![],
        });
    }
    rgb.devices.iter_mut().find(|d| d.device_id == dev_id).unwrap()
}

fn get_or_create_zone_config(dev: &mut RgbDeviceConfig, zone: u8) -> &mut RgbZoneConfig {
    if !dev.zones.iter().any(|z| z.zone_index == zone) {
        dev.zones.push(RgbZoneConfig {
            zone_index: zone,
            effect: RgbEffect {
                mode: RgbMode::Static,
                colors: vec![[255, 255, 255]],
                speed: 2,
                brightness: 4,
                direction: RgbDirection::Clockwise,
                scope: RgbScope::All,
            },
            swap_lr: false,
            swap_tb: false,
        });
    }
    dev.zones.iter_mut().find(|z| z.zone_index == zone).unwrap()
}

fn parse_rgb_mode(s: &str) -> RgbMode {
    // Match against Debug format of RgbMode variants
    match s {
        "Off" => RgbMode::Off,
        "Direct" => RgbMode::Direct,
        "Static" => RgbMode::Static,
        "Rainbow" => RgbMode::Rainbow,
        "RainbowMorph" => RgbMode::RainbowMorph,
        "Breathing" => RgbMode::Breathing,
        "Runway" => RgbMode::Runway,
        "Meteor" => RgbMode::Meteor,
        "ColorCycle" => RgbMode::ColorCycle,
        "Staggered" => RgbMode::Staggered,
        "Tide" => RgbMode::Tide,
        "Mixing" => RgbMode::Mixing,
        "Voice" => RgbMode::Voice,
        "Door" => RgbMode::Door,
        "Render" => RgbMode::Render,
        "Ripple" => RgbMode::Ripple,
        "Reflect" => RgbMode::Reflect,
        "TailChasing" => RgbMode::TailChasing,
        "Paint" => RgbMode::Paint,
        "PingPong" => RgbMode::PingPong,
        "Stack" => RgbMode::Stack,
        "CoverCycle" => RgbMode::CoverCycle,
        "Wave" => RgbMode::Wave,
        "Racing" => RgbMode::Racing,
        "Lottery" => RgbMode::Lottery,
        "Intertwine" => RgbMode::Intertwine,
        "MeteorShower" => RgbMode::MeteorShower,
        "Collide" => RgbMode::Collide,
        "ElectricCurrent" => RgbMode::ElectricCurrent,
        "Kaleidoscope" => RgbMode::Kaleidoscope,
        "BigBang" => RgbMode::BigBang,
        "Vortex" => RgbMode::Vortex,
        "Pump" => RgbMode::Pump,
        "ColorsMorph" => RgbMode::ColorsMorph,
        _ => RgbMode::Off,
    }
}

fn parse_rgb_direction(s: &str) -> RgbDirection {
    match s {
        "Clockwise" => RgbDirection::Clockwise,
        "CounterClockwise" => RgbDirection::CounterClockwise,
        "Up" => RgbDirection::Up,
        "Down" => RgbDirection::Down,
        "Spread" => RgbDirection::Spread,
        "Gather" => RgbDirection::Gather,
        _ => RgbDirection::Clockwise,
    }
}

fn parse_rgb_scope(s: &str) -> RgbScope {
    match s {
        "All" => RgbScope::All,
        "Top" => RgbScope::Top,
        "Bottom" => RgbScope::Bottom,
        "Inner" => RgbScope::Inner,
        "Outer" => RgbScope::Outer,
        _ => RgbScope::All,
    }
}
