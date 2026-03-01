//! Background thread: polls daemon, dispatches IPC commands, pushes state to UI.

use crate::ipc_client;
use crate::conversions;
use lianli_shared::config::AppConfig;
use lianli_shared::ipc::{DeviceInfo, IpcRequest, TelemetrySnapshot};
use lianli_shared::rgb::RgbDeviceCapabilities;
use std::sync::mpsc;
use std::time::Duration;

/// Commands sent from the UI thread to the backend.
#[derive(Debug)]
pub enum BackendCommand {
    RefreshDevices,
    SaveConfig(AppConfig),
    /// Send an arbitrary IPC request (fire-and-forget).
    IpcRequest(IpcRequest),
    Shutdown,
}

/// Handle for communicating with the backend thread.
pub struct BackendHandle {
    pub tx: mpsc::Sender<BackendCommand>,
}

impl BackendHandle {
    pub fn send(&self, cmd: BackendCommand) {
        let _ = self.tx.send(cmd);
    }
}

/// Start the backend thread. Returns a handle for sending commands.
pub fn start(window: slint::Weak<crate::MainWindow>) -> BackendHandle {
    let (tx, rx) = mpsc::channel::<BackendCommand>();

    std::thread::spawn(move || {
        run_backend(window, rx);
    });

    BackendHandle { tx }
}

fn run_backend(
    window: slint::Weak<crate::MainWindow>,
    rx: mpsc::Receiver<BackendCommand>,
) {
    let poll_interval = Duration::from_secs(2);

    // Initial load
    poll_daemon(&window);
    load_config(&window);

    loop {
        // Check for commands (non-blocking with timeout = poll interval)
        match rx.recv_timeout(poll_interval) {
            Ok(BackendCommand::RefreshDevices) => {
                poll_daemon(&window);
            }
            Ok(BackendCommand::SaveConfig(config)) => {
                save_config(&window, config);
                // Reload so UI reflects saved state
                load_config(&window);
            }
            Ok(BackendCommand::IpcRequest(req)) => {
                if let Err(e) = ipc_client::send_request(&req) {
                    tracing::error!("IPC request failed: {e}");
                }
            }
            Ok(BackendCommand::Shutdown) => {
                tracing::info!("Backend thread shutting down");
                break;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // Regular poll
                poll_daemon(&window);
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                tracing::info!("Backend channel disconnected, shutting down");
                break;
            }
        }
    }
}

/// Poll daemon for device list and telemetry, push to UI.
fn poll_daemon(window: &slint::Weak<crate::MainWindow>) {
    let connected = ipc_client::is_daemon_running();

    let (devices, telemetry) = if connected {
        let devices: Vec<DeviceInfo> = ipc_client::send_request(&IpcRequest::ListDevices)
            .and_then(ipc_client::unwrap_response)
            .unwrap_or_default();

        let telemetry: TelemetrySnapshot = ipc_client::send_request(&IpcRequest::GetTelemetry)
            .and_then(ipc_client::unwrap_response)
            .unwrap_or_default();

        (devices, telemetry)
    } else {
        (Vec::new(), TelemetrySnapshot::default())
    };

    let device_count = devices.iter().filter(|d| !matches!(
        d.family,
        lianli_shared::device_id::DeviceFamily::WirelessTx
        | lianli_shared::device_id::DeviceFamily::WirelessRx
        | lianli_shared::device_id::DeviceFamily::DisplaySwitcher
    )).count() as i32;

    let streaming_active = telemetry.streaming_active;
    let openrgb_running = telemetry.openrgb_status.running;
    let openrgb_error = telemetry.openrgb_status.error.clone().unwrap_or_default();
    let socket_path = ipc_client::SOCKET_PATH.clone();

    let window = window.clone();
    slint::invoke_from_event_loop(move || {
        if let Some(w) = window.upgrade() {
            w.set_daemon_connected(connected);
            w.set_device_count(device_count);
            w.set_streaming_active(streaming_active);
            w.set_socket_path(slint::SharedString::from(&socket_path));
            w.set_openrgb_running(openrgb_running);
            w.set_openrgb_error(slint::SharedString::from(&openrgb_error));
            // Build model on UI thread (ModelRc is !Send)
            let model = conversions::devices_to_model(&devices, &telemetry);
            w.set_devices(model);
        }
    })
    .ok();
}

/// Load config and RGB capabilities from daemon, push all to UI.
fn load_config(window: &slint::Weak<crate::MainWindow>) {
    let config: Option<AppConfig> = ipc_client::send_request(&IpcRequest::GetConfig)
        .and_then(ipc_client::unwrap_response)
        .ok();

    let rgb_caps: Vec<RgbDeviceCapabilities> =
        ipc_client::send_request(&IpcRequest::GetRgbCapabilities)
            .and_then(ipc_client::unwrap_response)
            .unwrap_or_default();

    let devices: Vec<DeviceInfo> = ipc_client::send_request(&IpcRequest::ListDevices)
        .and_then(ipc_client::unwrap_response)
        .unwrap_or_default();

    if let Some(config) = config {
        let lcd_count = config.lcds.len() as i32;
        let fan_curve_count = config.fan_curves.len() as i32;
        let default_fps = config.default_fps as i32;
        let rgb = config.rgb.clone().unwrap_or_default();
        let openrgb_enabled = rgb.openrgb_server;
        let openrgb_port = rgb.openrgb_port as i32;
        let fan_update_interval = config.fans.as_ref()
            .map(|f| f.update_interval_ms as i32)
            .unwrap_or(1000);

        let window = window.clone();
        slint::invoke_from_event_loop(move || {
            if let Some(w) = window.upgrade() {
                w.set_has_config(true);
                w.set_config_dirty(false);
                w.set_lcd_count(lcd_count);
                w.set_fan_curve_count(fan_curve_count);
                w.set_default_fps(default_fps);
                w.set_openrgb_enabled(openrgb_enabled);
                w.set_openrgb_port(openrgb_port);

                // LCD entries
                let lcd_model = conversions::lcd_entries_to_model(&config.lcds);
                w.set_lcd_entries(lcd_model);
                let lcd_opts = conversions::lcd_device_options(&devices);
                w.set_lcd_device_options(lcd_opts);

                // Fan curves — use approximate plot dimensions for pixel coords
                // (actual plot is dynamic, but points are relative to these)
                let plot_w = 400.0;
                let plot_h = 160.0;
                let curves_model = conversions::fan_curves_to_model(&config.fan_curves, plot_w, plot_h);
                w.set_fan_curves(curves_model);
                let names_model = conversions::curve_names_to_model(&config.fan_curves);
                w.set_curve_names(names_model);
                let speed_opts = conversions::speed_options_model(&config.fan_curves, true);
                w.set_fan_speed_options(speed_opts);
                w.set_fan_update_interval(fan_update_interval);

                // Fan groups
                if let Some(ref fan_cfg) = config.fans {
                    let groups_model = conversions::fan_groups_to_model(fan_cfg, &devices);
                    w.set_fan_groups(groups_model);
                }

                // RGB devices
                let rgb_model = conversions::rgb_devices_to_model(&rgb_caps, &config);
                w.set_rgb_devices(rgb_model);
            }
        })
        .ok();
    }
}

/// Save config to daemon.
fn save_config(window: &slint::Weak<crate::MainWindow>, config: AppConfig) {
    let result = ipc_client::send_request(&IpcRequest::SetConfig { config });

    let window = window.clone();
    slint::invoke_from_event_loop(move || {
        if let Some(w) = window.upgrade() {
            match result {
                Ok(_) => {
                    w.set_config_dirty(false);
                    tracing::info!("Config saved successfully");
                }
                Err(e) => {
                    tracing::error!("Failed to save config: {e}");
                }
            }
        }
    })
    .ok();
}
