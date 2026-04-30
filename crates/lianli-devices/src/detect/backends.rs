use super::enumerate::{find_hid_devices_by_family, find_usb_device};
use super::{DetectedDevice, DetectedHidDevice};
use anyhow::Result;
use hidapi::HidApi;
use lianli_shared::device_id::DeviceFamily;
use lianli_transport::{HidBackend, HidBackendKind, HidReopener, RusbHidTransport};
use parking_lot::Mutex;
use rusb::{Device, GlobalContext};
use std::sync::Arc;
use std::time::Duration;
use tracing::warn;

/// Build a reopener that re-acquires the same HID device via hidapi by VID/PID.
/// Used to recover from stale handles after USB suspend/resume.
fn make_hidapi_reopener(vid: u16, pid: u16, family: DeviceFamily) -> HidReopener {
    Arc::new(move || {
        let api = HidApi::new().map_err(|e| anyhow::anyhow!("hidapi init: {e}"))?;
        let det = find_hid_devices_by_family(&api, family)
            .into_iter()
            .find(|d| d.vid == vid && d.pid == pid)
            .ok_or_else(|| {
                anyhow::anyhow!("HID device {vid:04x}:{pid:04x} not enumerable on reopen")
            })?;
        let dev = api
            .open_path(&det.path)
            .map_err(|e| anyhow::anyhow!("hidapi open_path: {e}"))?;
        Ok(HidBackendKind::Hidapi(dev))
    })
}

/// Build a reopener that re-acquires the same HID device via the rusb backend.
fn make_rusb_reopener(vid: u16, pid: u16, usage_page: Option<u16>) -> HidReopener {
    Arc::new(move || {
        let usb_dev = find_usb_device(vid, pid).ok_or_else(|| {
            anyhow::anyhow!("USB device {vid:04x}:{pid:04x} not enumerable on reopen")
        })?;
        let transport = RusbHidTransport::open_by_usage(usb_dev, usage_page)
            .map_err(|e| anyhow::anyhow!("rusb hid open: {e}"))?;
        Ok(HidBackendKind::Rusb(transport))
    })
}

/// Try opening a device, retrying on failure. First two retries are plain
/// reopens, only the last retry does a USB port reset.
fn try_open_with_retry<T>(
    usb_device: Option<&Device<GlobalContext>>,
    label: &str,
    mut open_fn: impl FnMut() -> Result<T>,
) -> Result<T> {
    const MAX_RETRIES: u32 = 3;
    const RESET_AT: u32 = 2;
    for attempt in 0..=MAX_RETRIES {
        match open_fn() {
            Ok(t) => return Ok(t),
            Err(e) if attempt < MAX_RETRIES => {
                if attempt == RESET_AT {
                    if let Some(usb_dev) = usb_device {
                        warn!(
                            "{label}: open attempt {} failed: {e}, resetting USB device",
                            attempt + 1
                        );
                        let _ = RusbHidTransport::reset_usb_device(usb_dev);
                        std::thread::sleep(Duration::from_secs(3));
                    } else {
                        return Err(e.context(format!(
                            "{label}: failed and no USB device available for reset"
                        )));
                    }
                } else {
                    warn!(
                        "{label}: open attempt {} failed: {e}, retrying",
                        attempt + 1
                    );
                    std::thread::sleep(Duration::from_millis(250));
                }
            }
            Err(e) => {
                return Err(e.context(format!(
                    "{label}: failed after {} attempts",
                    MAX_RETRIES + 1
                )));
            }
        }
    }
    unreachable!()
}

fn open_with_retry<T>(
    usb_device: &Device<GlobalContext>,
    open_fn: impl FnMut() -> Result<T>,
) -> Result<T> {
    try_open_with_retry(Some(usb_device), "rusb open", open_fn)
}

/// Open a detected HID device as an LCD controller via hidapi.
pub fn open_hid_lcd_device(
    api: &HidApi,
    det: &DetectedHidDevice,
) -> Option<Result<Box<dyn crate::traits::LcdDevice>>> {
    let pid = det.pid;
    match det.family {
        DeviceFamily::HydroShiftLcd | DeviceFamily::Galahad2Lcd => {
            Some(open_hidapi_with_retry(api, det, |backend| {
                let backend = Arc::new(Mutex::new(backend));
                crate::hydroshift_lcd::HydroShiftLcdController::new(backend, pid)
                    .map(|d| Box::new(d) as Box<dyn crate::traits::LcdDevice>)
            }))
        }
        DeviceFamily::TlLcd => Some(open_hidapi_with_retry(api, det, |backend| {
            let backend = Arc::new(Mutex::new(backend));
            let mut tl = crate::tl_lcd::TlLcdDevice::new(backend);
            crate::traits::LcdDevice::initialize(&mut tl)?;
            Ok(Box::new(tl) as Box<dyn crate::traits::LcdDevice>)
        })),
        _ => None,
    }
}

/// Open a HID LCD device by VID/PID using hidapi with retry logic.
///
/// Unlike `open_hid_lcd_device` (which requires a pre-enumerated `DetectedHidDevice`),
/// this function handles the case where no hidraw node exists yet by performing
/// USB reset + re-enumeration before retrying.
pub fn open_hid_lcd_by_vid_pid(
    vid: u16,
    pid: u16,
    family: DeviceFamily,
) -> Result<Box<dyn crate::traits::LcdDevice>> {
    let usb_device = find_usb_device(vid, pid);

    for attempt in 0..=3u32 {
        let api = HidApi::new().map_err(|e| anyhow::anyhow!("hidapi init: {e}"))?;
        let hid_devs = find_hid_devices_by_family(&api, family);

        if let Some(det) = hid_devs.into_iter().next() {
            match open_hid_lcd_device(&api, &det) {
                Some(Ok(ctrl)) => return Ok(ctrl),
                Some(Err(e)) if attempt < 3 => {
                    warn!(
                        "HID LCD open attempt {} failed ({vid:04x}:{pid:04x}): {e}, resetting USB",
                        attempt + 1
                    );
                }
                Some(Err(e)) => {
                    return Err(e.context("HID LCD open failed after 4 attempts"));
                }
                None => {
                    return Err(anyhow::anyhow!("family does not support LCD"));
                }
            }
        } else if attempt < 3 {
            warn!(
                "No hidraw node for {:04x}:{:04x} (attempt {}), resetting USB",
                vid,
                pid,
                attempt + 1
            );
        } else {
            return Err(anyhow::anyhow!(
                "no HID device found for {vid:04x}:{pid:04x} after 4 attempts"
            ));
        }

        if let Some(ref usb_dev) = usb_device {
            let _ = RusbHidTransport::reset_usb_device(usb_dev);
            std::thread::sleep(Duration::from_secs(3));
        } else {
            return Err(anyhow::anyhow!(
                "no USB device found for reset ({vid:04x}:{pid:04x})"
            ));
        }
    }
    unreachable!()
}

/// Open a detected HID device as an LCD controller via rusb.
pub fn open_hid_lcd_device_rusb(
    det: &DetectedDevice,
) -> Option<Result<Box<dyn crate::traits::LcdDevice>>> {
    match det.family {
        DeviceFamily::HydroShiftLcd | DeviceFamily::Galahad2Lcd => {
            let pid = det.pid;
            Some(open_with_retry(&det.device, || {
                let transport =
                    RusbHidTransport::open_by_usage(det.device.clone(), det.hid_usage_page)?;
                let mut backend = HidBackend::from_rusb(transport)
                    .with_reopener(make_rusb_reopener(det.vid, det.pid, det.hid_usage_page));
                backend.read_flush();
                let backend = Arc::new(Mutex::new(backend));
                crate::hydroshift_lcd::HydroShiftLcdController::new(backend, pid)
                    .map(|d| Box::new(d) as Box<dyn crate::traits::LcdDevice>)
            }))
        }
        DeviceFamily::TlLcd => Some(open_with_retry(&det.device, || {
            let transport =
                RusbHidTransport::open_by_usage(det.device.clone(), det.hid_usage_page)?;
            let backend = HidBackend::from_rusb(transport).with_reopener(make_rusb_reopener(
                det.vid,
                det.pid,
                det.hid_usage_page,
            ));
            let backend = Arc::new(Mutex::new(backend));
            let mut tl = crate::tl_lcd::TlLcdDevice::new(backend);
            crate::traits::LcdDevice::initialize(&mut tl)?;
            Ok(Box::new(tl) as Box<dyn crate::traits::LcdDevice>)
        })),
        _ => None,
    }
}

/// Wrap hidapi open with retry logic. On failure, performs USB reset and retries.
pub fn open_hidapi_with_retry<T>(
    api: &HidApi,
    det: &DetectedHidDevice,
    mut create_fn: impl FnMut(HidBackend) -> Result<T>,
) -> Result<T> {
    let usb_device = find_usb_device(det.vid, det.pid);
    let label = format!("HID open {} ({:04x}:{:04x})", det.name, det.vid, det.pid);

    let backend = try_open_with_retry(usb_device.as_ref(), &label, || {
        let hid_dev = api
            .open_path(&det.path)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        let mut backend = HidBackend::from_hidapi(hid_dev)
            .with_reopener(make_hidapi_reopener(det.vid, det.pid, det.family));
        backend.read_flush();
        Ok(backend)
    })?;
    create_fn(backend)
}

/// Open a shared HID backend via hidapi with retry logic.
/// Returns an `Arc<Mutex<HidBackend>>` that can be shared between multiple controllers.
pub fn open_hid_backend_hidapi(
    api: &HidApi,
    det: &DetectedHidDevice,
) -> Result<Arc<Mutex<HidBackend>>> {
    open_hidapi_with_retry(api, det, |backend| Ok(Arc::new(Mutex::new(backend))))
}

/// Open a shared HID backend via rusb with retry logic.
/// Returns an `Arc<Mutex<HidBackend>>` that can be shared between multiple controllers.
pub fn open_hid_backend_rusb(det: &DetectedDevice) -> Result<Arc<Mutex<HidBackend>>> {
    let vid = det.vid;
    let pid = det.pid;
    let usage_page = det.hid_usage_page;
    open_with_retry(&det.device, || {
        let transport = RusbHidTransport::open_by_usage(det.device.clone(), det.hid_usage_page)?;
        let backend = HidBackend::from_rusb(transport)
            .with_reopener(make_rusb_reopener(vid, pid, usage_page));
        Ok(Arc::new(Mutex::new(backend)))
    })
}
