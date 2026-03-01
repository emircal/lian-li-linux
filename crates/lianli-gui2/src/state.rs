//! Application state — replaces Pinia stores from the Vue GUI.

use lianli_shared::config::AppConfig;
use lianli_shared::ipc::{DeviceInfo, TelemetrySnapshot};
use lianli_shared::rgb::RgbDeviceCapabilities;

/// Centralized application state, updated by the backend thread.
#[derive(Debug, Default)]
#[allow(dead_code)]
pub struct AppState {
    // Device store
    pub devices: Vec<DeviceInfo>,
    pub telemetry: TelemetrySnapshot,
    pub daemon_connected: bool,

    // Config store
    pub config: Option<AppConfig>,
    pub config_dirty: bool,

    // RGB
    pub rgb_capabilities: Vec<RgbDeviceCapabilities>,
}
