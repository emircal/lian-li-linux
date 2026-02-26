use anyhow::Result;
use hidapi::HidApi;
use lianli_shared::device_id::{uses_hid, DeviceFamily, KNOWN_DEVICES, UsbId};
use rusb::{Device, GlobalContext};
use std::collections::HashSet;
use tracing::debug;

/// A detected USB device with its identified family.
#[derive(Debug)]
pub struct DetectedDevice {
    pub device: Device<GlobalContext>,
    pub family: DeviceFamily,
    pub name: &'static str,
    pub vid: u16,
    pub pid: u16,
    pub bus: u8,
    pub address: u8,
    pub serial: Option<String>,
}

/// A detected HID device with its identified family.
#[derive(Debug, Clone)]
pub struct DetectedHidDevice {
    pub family: DeviceFamily,
    pub name: &'static str,
    pub vid: u16,
    pub pid: u16,
    pub path: std::ffi::CString,
    pub serial: Option<String>,
}

/// Known non-unique HID serial strings (chip manufacturer names, not device serials).
const NON_UNIQUE_SERIALS: &[&str] = &["Nuvoton"];

impl DetectedHidDevice {
    /// Generate a stable device ID. Uses the serial if it's unique,
    /// otherwise falls back to VID:PID with a path-derived suffix to
    /// disambiguate multiple identical controllers.
    pub fn device_id(&self) -> String {
        match &self.serial {
            Some(s) if !NON_UNIQUE_SERIALS.contains(&s.as_str()) => s.clone(),
            _ => {
                // Use HID path bytes to create a short disambiguator
                let path_bytes = self.path.as_bytes();
                let hash: u32 = path_bytes
                    .iter()
                    .fold(0u32, |acc, &b| acc.wrapping_mul(31).wrapping_add(b as u32));
                format!("hid:{:04x}:{:04x}:{:04x}", self.vid, self.pid, hash as u16)
            }
        }
    }
}

/// Enumerate all Lian Li USB devices on the system.
pub fn enumerate_devices() -> Result<Vec<DetectedDevice>> {
    let usb_devices = rusb::devices()?;
    let mut found = Vec::new();

    for device in usb_devices.iter() {
        let desc = match device.device_descriptor() {
            Ok(d) => d,
            Err(_) => continue,
        };

        let vid = desc.vendor_id();
        let pid = desc.product_id();
        let id = UsbId::new(vid, pid);

        if let Some(entry) = KNOWN_DEVICES.iter().find(|e| e.id == id) {
            let bus = device.bus_number();
            let address = device.address();

            // Try to read serial number
            let serial = device
                .open()
                .ok()
                .and_then(|h| h.read_serial_number_string_ascii(&desc).ok());

            debug!(
                "Found {} ({:04x}:{:04x}) at bus {} addr {} serial {:?}",
                entry.name, vid, pid, bus, address, serial
            );

            found.push(DetectedDevice {
                device,
                family: entry.family,
                name: entry.name,
                vid,
                pid,
                bus,
                address,
                serial,
            });
        }
    }

    found.sort_by_key(|d| (d.bus, d.address));
    Ok(found)
}

/// Find all USB devices matching a specific family.
pub fn find_devices_by_family(family: DeviceFamily) -> Result<Vec<DetectedDevice>> {
    Ok(enumerate_devices()?
        .into_iter()
        .filter(|d| d.family == family)
        .collect())
}

/// Enumerate all known Lian Li HID devices.
///
/// When a device entry specifies `hid_usage_page`, only the HID interface
/// matching that usage page is returned. Otherwise, deduplicates by
/// vid:pid:serial to avoid opening the wrong interface.
pub fn enumerate_hid_devices(api: &HidApi) -> Vec<DetectedHidDevice> {
    let mut found = Vec::new();
    let mut seen = HashSet::new();

    for dev_info in api.device_list() {
        let vid = dev_info.vendor_id();
        let pid = dev_info.product_id();
        let id = UsbId::new(vid, pid);

        if let Some(entry) = KNOWN_DEVICES.iter().find(|e| e.id == id) {
            if !uses_hid(entry.family) {
                continue;
            }

            // Filter by usage page if the device requires a specific HID interface
            if let Some(required_page) = entry.hid_usage_page {
                if dev_info.usage_page() != required_page {
                    continue;
                }
            } else {
                // No usage page filter — deduplicate by vid:pid:serial
                // to avoid opening the wrong HID interface.
                // (TL Fan uses usage_page filtering so this branch only
                // applies to ENE and other devices with unique serials.)
                let serial_str = dev_info
                    .serial_number()
                    .unwrap_or("")
                    .to_string();
                let dedup_key = (vid, pid, serial_str);
                if !seen.insert(dedup_key) {
                    continue;
                }
            }

            let serial = dev_info
                .serial_number()
                .map(|s| s.to_string());

            debug!(
                "Found HID {} ({:04x}:{:04x}) usage_page={:#06x} path={:?} serial={:?}",
                entry.name,
                vid,
                pid,
                dev_info.usage_page(),
                dev_info.path(),
                serial
            );

            found.push(DetectedHidDevice {
                family: entry.family,
                name: entry.name,
                vid,
                pid,
                path: dev_info.path().to_owned(),
                serial,
            });
        }
    }

    found
}

/// Find HID devices matching a specific family.
pub fn find_hid_devices_by_family(api: &HidApi, family: DeviceFamily) -> Vec<DetectedHidDevice> {
    enumerate_hid_devices(api)
        .into_iter()
        .filter(|d| d.family == family)
        .collect()
}

/// Open a detected HID device as a fan controller.
///
/// Returns `None` if the family doesn't support fan control via HID,
/// or `Err` if opening/init fails.
pub fn open_fan_device(
    api: &HidApi,
    det: &DetectedHidDevice,
) -> Option<Result<Box<dyn crate::traits::FanDevice>>> {
    if !det.family.has_fan() {
        return None;
    }
    match det.family {
        DeviceFamily::TlFan => {
            let hid_dev = match api.open_path(&det.path) {
                Ok(d) => d,
                Err(e) => return Some(Err(anyhow::anyhow!("HID open: {e}"))),
            };
            Some(
                crate::tl_fan::TlFanController::new(hid_dev)
                    .map(|c| Box::new(c) as Box<dyn crate::traits::FanDevice>),
            )
        }
        DeviceFamily::Ene6k77 => {
            let hid_dev = match api.open_path(&det.path) {
                Ok(d) => d,
                Err(e) => return Some(Err(anyhow::anyhow!("HID open: {e}"))),
            };
            Some(
                crate::ene6k77::Ene6k77Controller::new(hid_dev, det.pid)
                    .map(|c| Box::new(c) as Box<dyn crate::traits::FanDevice>),
            )
        }
        // Wireless fan devices are handled separately (not HID-based)
        _ => None,
    }
}

/// Open a detected HID device as RGB controller(s).
///
/// Returns a list of `(device_id_suffix, RgbDevice)` pairs.
/// For TL Fan controllers, each active port becomes a separate device (suffix = "port0", "port1", etc.).
/// For other devices, a single device is returned with an empty suffix.
/// Returns `None` if the family doesn't support RGB control via HID.
/// Opens a separate HID handle so it can coexist with fan control.
pub fn open_rgb_devices(
    api: &HidApi,
    det: &DetectedHidDevice,
) -> Option<Result<Vec<(String, Box<dyn crate::traits::RgbDevice>)>>> {
    if !det.family.has_rgb() {
        return None;
    }
    match det.family {
        DeviceFamily::TlFan => {
            let hid_dev = match api.open_path(&det.path) {
                Ok(d) => d,
                Err(e) => return Some(Err(anyhow::anyhow!("HID open for RGB: {e}"))),
            };
            Some(
                crate::tl_fan::TlFanController::new(hid_dev).map(|ctrl| {
                    ctrl.into_port_devices()
                        .into_iter()
                        .map(|(port, dev)| {
                            (format!("port{port}"), Box::new(dev) as Box<dyn crate::traits::RgbDevice>)
                        })
                        .collect()
                }),
            )
        }
        DeviceFamily::Ene6k77 => {
            let hid_dev = match api.open_path(&det.path) {
                Ok(d) => d,
                Err(e) => return Some(Err(anyhow::anyhow!("HID open for RGB: {e}"))),
            };
            Some(
                crate::ene6k77::Ene6k77Controller::new(hid_dev, det.pid)
                    .map(|c| vec![(String::new(), Box::new(c) as Box<dyn crate::traits::RgbDevice>)]),
            )
        }
        DeviceFamily::Galahad2Trinity => {
            let hid_dev = match api.open_path(&det.path) {
                Ok(d) => d,
                Err(e) => return Some(Err(anyhow::anyhow!("HID open for RGB: {e}"))),
            };
            Some(
                crate::galahad2_trinity::Galahad2TrinityController::new(hid_dev, det.pid)
                    .map(|c| vec![(String::new(), Box::new(c) as Box<dyn crate::traits::RgbDevice>)]),
            )
        }
        DeviceFamily::HydroShiftLcd | DeviceFamily::Galahad2Lcd => {
            let hid_dev = match api.open_path(&det.path) {
                Ok(d) => d,
                Err(e) => return Some(Err(anyhow::anyhow!("HID open for RGB: {e}"))),
            };
            Some(
                crate::hydroshift_lcd::AioLcdRgbController::new(hid_dev, det.pid)
                    .map(|c| vec![(String::new(), Box::new(c) as Box<dyn crate::traits::RgbDevice>)]),
            )
        }
        _ => None,
    }
}

/// Find LCD devices (SLV3/TLV2 wireless LCD fans via USB bulk).
pub fn find_wireless_lcd_devices() -> Result<Vec<Device<GlobalContext>>> {
    let devices = rusb::devices()?;
    let lcd_pids: &[(u16, u16)] = &[
        (0x1CBE, 0x0005), // SLV3
        (0x1CBE, 0x0006), // TLV2
    ];
    let mut list = Vec::new();
    for device in devices.iter() {
        if let Ok(desc) = device.device_descriptor() {
            let vid = desc.vendor_id();
            let pid = desc.product_id();
            if lcd_pids.iter().any(|&(v, p)| v == vid && p == pid) {
                list.push(device);
            }
        }
    }
    list.sort_by_key(|dev| (dev.bus_number(), dev.address()));
    Ok(list)
}
