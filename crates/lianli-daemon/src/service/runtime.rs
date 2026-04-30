use super::DaemonEvent;
use lianli_devices::crypto::PacketBuilder;
use lianli_devices::slv3_lcd::Slv3LcdDevice;
use lianli_devices::traits::LcdDevice;
use lianli_devices::winusb_lcd::WinUsbLcdDevice;
use lianli_devices::wireless::WirelessController;
use lianli_media::sensor::FrameInfo;
use lianli_media::{CustomAsset, MediaAsset, MediaAssetKind, SensorAsset};
use lianli_shared::config::ConfigKey;
use parking_lot::Mutex;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

pub(super) enum LcdBackend {
    Slv3(Slv3LcdDevice),
    WinUsb(ThreadedWinUsbSender),
    HidLcd(Box<dyn LcdDevice>),
}

impl LcdBackend {
    fn send_frame(
        &mut self,
        wireless: &WirelessController,
        builder: &mut PacketBuilder,
        frame: &[u8],
    ) -> anyhow::Result<()> {
        match self {
            Self::Slv3(d) => {
                wireless.ensure_video_mode()?;
                d.send_frame(builder, frame)
            }
            Self::WinUsb(d) => d.send_frame(frame),
            Self::HidLcd(d) => d.send_jpeg_frame(frame),
        }
    }

    fn send_frame_verified(
        &mut self,
        wireless: &WirelessController,
        builder: &mut PacketBuilder,
        frame: &[u8],
    ) -> anyhow::Result<()> {
        match self {
            Self::WinUsb(d) => d.send_frame_verified(frame),
            Self::HidLcd(d) => d.send_static_frame(frame),
            _ => self.send_frame(wireless, builder, frame),
        }
    }
}

enum LcdThreadMsg {
    Frame(Vec<u8>),
    FrameVerified(Vec<u8>, std::sync::mpsc::SyncSender<anyhow::Result<()>>),
    StreamH264 { path: PathBuf, looping: bool },
    SwitchDesktop(std::sync::mpsc::SyncSender<anyhow::Result<()>>),
    Stop,
}

pub(super) struct ThreadedWinUsbSender {
    tx: std::sync::mpsc::SyncSender<LcdThreadMsg>,
    h264_stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl ThreadedWinUsbSender {
    pub(super) fn new(mut device: WinUsbLcdDevice, index: usize) -> Self {
        let (tx, rx) = std::sync::mpsc::sync_channel::<LcdThreadMsg>(2);
        let h264_stop = Arc::new(AtomicBool::new(false));
        let stop_clone = Arc::clone(&h264_stop);
        let thread = thread::spawn(move || {
            for msg in rx {
                match msg {
                    LcdThreadMsg::Frame(data) => {
                        if let Err(e) = device.send_frame(&data) {
                            warn!("LCD[{index}] sender thread frame error: {e}");
                        }
                    }
                    LcdThreadMsg::FrameVerified(data, reply) => {
                        let result = device.send_frame_verified(&data);
                        let _ = reply.send(result);
                    }
                    LcdThreadMsg::StreamH264 { path, looping } => {
                        stop_clone.store(false, Ordering::Relaxed);
                        if let Err(e) = device.stream_h264(&path, looping, &stop_clone) {
                            warn!("LCD[{index}] h264 stream error: {e}");
                        }
                    }
                    LcdThreadMsg::SwitchDesktop(reply) => {
                        let result = device.switch_to_desktop_mode();
                        let _ = reply.send(result);
                        break;
                    }
                    LcdThreadMsg::Stop => break,
                }
            }
            device.transport_release();
        });
        Self {
            tx,
            h264_stop,
            thread: Some(thread),
        }
    }

    fn stream_h264(&self, path: PathBuf, looping: bool) -> anyhow::Result<()> {
        self.h264_stop.store(true, Ordering::Relaxed);
        self.tx
            .send(LcdThreadMsg::StreamH264 { path, looping })
            .map_err(|_| anyhow::anyhow!("LCD sender thread exited"))?;
        Ok(())
    }

    fn send_frame(&self, frame: &[u8]) -> anyhow::Result<()> {
        self.h264_stop.store(true, Ordering::Relaxed);
        match self.tx.try_send(LcdThreadMsg::Frame(frame.to_vec())) {
            Ok(()) => Ok(()),
            Err(std::sync::mpsc::TrySendError::Full(_)) => {
                debug!("LCD sender busy, dropping frame");
                Ok(())
            }
            Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                anyhow::bail!("LCD sender thread exited")
            }
        }
    }

    pub(super) fn switch_to_desktop_mode(&mut self) -> anyhow::Result<()> {
        self.h264_stop.store(true, Ordering::Relaxed);
        let (reply_tx, reply_rx) = std::sync::mpsc::sync_channel(1);
        self.tx
            .send(LcdThreadMsg::SwitchDesktop(reply_tx))
            .map_err(|_| anyhow::anyhow!("LCD sender thread exited"))?;
        let result = reply_rx
            .recv_timeout(Duration::from_secs(10))
            .map_err(|_| anyhow::anyhow!("LCD sender thread timeout"))?;
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        result
    }

    fn send_frame_verified(&self, frame: &[u8]) -> anyhow::Result<()> {
        self.h264_stop.store(true, Ordering::Relaxed);
        let (reply_tx, reply_rx) = std::sync::mpsc::sync_channel(1);
        self.tx
            .send(LcdThreadMsg::FrameVerified(frame.to_vec(), reply_tx))
            .map_err(|_| anyhow::anyhow!("LCD sender thread exited"))?;
        reply_rx
            .recv_timeout(Duration::from_secs(10))
            .map_err(|_| anyhow::anyhow!("LCD sender thread timeout"))?
    }

    fn stop(&mut self) {
        self.h264_stop.store(true, Ordering::Relaxed);
        let _ = self.tx.send(LcdThreadMsg::Stop);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for ThreadedWinUsbSender {
    fn drop(&mut self) {
        self.stop();
    }
}

pub(crate) struct ActiveTarget {
    pub(super) index: usize,
    pub(super) key: ConfigKey,
    pub(super) device_identity: String,
    pub(super) lcd: LcdBackend,
    media: MediaRuntime,
    pub(super) asset: Arc<MediaAsset>,
    // This variable contains the last seen frame version. Each renderer holds a frame version counter which gets increased each time it actually writes into the frame. The first time it writes into the frame sets the frame version to 1
    // By using this mechanism we are able to detect whether we actually need to send the frame via USB bus to the LCD, and thus we can save quite a lot of time by not sending frames which are already displayed.
    pub(super) frame_counter: u64,
    pub(super) consecutive_errors: u32,
}

impl ActiveTarget {
    pub(super) fn new(
        index: usize,
        key: ConfigKey,
        device_identity: String,
        lcd: LcdBackend,
        asset: Arc<MediaAsset>,
        tx: Option<Sender<DaemonEvent>>,
    ) -> Self {
        Self {
            index,
            key,
            device_identity,
            lcd,
            media: MediaRuntime::from_asset(Arc::clone(&asset), tx),
            asset,
            frame_counter: 0,
            consecutive_errors: 0,
        }
    }

    pub(super) fn matches(&self, identity: &str, key: &ConfigKey) -> bool {
        self.device_identity == identity && key == &self.key
    }

    /// Replace the media asset without reopening the LCD transport.
    pub(super) fn swap_media(&mut self, asset: Arc<MediaAsset>, tx: Option<Sender<DaemonEvent>>) {
        self.asset = Arc::clone(&asset);
        self.media = MediaRuntime::from_asset(asset, tx);
        self.frame_counter = 0;
        info!(
            "[devices] LCD[{}] media swapped (keeping transport)",
            self.index
        );
    }

    pub(super) fn send_frame(
        &mut self,
        wireless: &WirelessController,
        builder: &mut PacketBuilder,
    ) -> Result<bool, SendError> {
        // H264: start the stream on the LCD thread, then it runs autonomously
        if let MediaRuntime::H264 {
            path,
            looping,
            started,
        } = &mut self.media
        {
            if !*started {
                if let LcdBackend::WinUsb(ref sender) = self.lcd {
                    sender
                        .stream_h264(path.clone(), *looping)
                        .map_err(|e| SendError::Other(e))?;
                    *started = true;
                }
            }
            return Ok(true);
        }

        let is_static = matches!(self.media, MediaRuntime::Static { .. });
        let frame = match self.media.next_frame_bytes() {
            Some(bytes) => bytes,
            None => return Ok(false),
        };

        let result = if is_static {
            self.lcd.send_frame_verified(wireless, builder, frame)
        } else {
            self.lcd.send_frame(wireless, builder, frame)
        };
        result.map_err(
            |err| match err.downcast::<lianli_transport::TransportError>() {
                Ok(usb) => SendError::Usb(usb),
                Err(other) => SendError::Other(other),
            },
        )?;

        self.frame_counter += 1;
        Ok(true)
    }

    pub(super) fn stop(&mut self) {}
}

enum MediaRuntime {
    Static {
        frame: Arc<Vec<u8>>,
    },
    Video {
        #[allow(dead_code)]
        player: Arc<AsyncVideoPlayer>,
        frames: Arc<Vec<Vec<u8>>>,
        sent_frame_index: usize,
    },
    Sensor {
        renderer: Arc<AsyncSensorRenderer>,
        cached_frame: Vec<u8>,
        sent_frame_index: usize,
    },
    H264 {
        path: PathBuf,
        looping: bool,
        started: bool,
    },
    Custom {
        renderer: Arc<AsyncCustomRenderer>,
        cached_frame: Vec<u8>,
        sent_frame_index: usize,
    },
}

struct AsyncSensorRenderer {
    #[allow(dead_code)] // We'd like to keep the SensorAsset, who knows if we'll need it
    asset: Arc<SensorAsset>,
    current_frame: Arc<Mutex<FrameInfo>>,
    stop_flag: Arc<AtomicBool>,
    _thread: Option<JoinHandle<()>>,
}

impl AsyncSensorRenderer {
    fn new(
        tx: Option<Sender<DaemonEvent>>,
        asset: Arc<SensorAsset>,
        baseasset: Arc<MediaAsset>,
    ) -> Self {
        let initial = match asset.render_frame(true) {
            Ok(Some(frame)) => frame,
            Ok(None) => asset.blank_frame(),
            Err(err) => {
                warn!("sensor initial render failed: {err}");
                asset.blank_frame()
            }
        };

        let current_frame = Arc::new(Mutex::new(initial));
        let stop_flag = Arc::new(AtomicBool::new(false));
        let update_interval = asset.update_interval();

        let asset_clone = Arc::clone(&asset);
        let frame_clone = Arc::clone(&current_frame);
        let stop_clone = Arc::clone(&stop_flag);

        let asset_for_thread = Arc::clone(&baseasset);
        let tx_for_thread = tx.clone();

        let thread = thread::spawn(move || {
            while !stop_clone.load(Ordering::Relaxed) {
                thread::sleep(update_interval);
                if stop_clone.load(Ordering::Relaxed) {
                    break;
                }
                match asset_clone.render_frame(false) {
                    Ok(Some(new_frame)) => {
                        *frame_clone.lock() = new_frame;
                        if let Some(ref tx) = tx_for_thread {
                            let event = DaemonEvent::FrameFinished {
                                asset: Arc::clone(&asset_for_thread),
                            };
                            if tx.send(event).is_err() {
                                break;
                            }
                        }
                    }
                    Ok(None) => {}
                    Err(err) => {
                        warn!("sensor background render failed: {err}");
                    }
                }
            }
        });

        Self {
            asset,
            current_frame,
            stop_flag,
            _thread: Some(thread),
        }
    }

    fn get_frame_index(&self) -> usize {
        self.current_frame.lock().frame_index
    }

    fn get_current_frame(&self) -> Vec<u8> {
        self.current_frame.lock().data.clone()
    }
}

impl Drop for AsyncSensorRenderer {
    fn drop(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
    }
}

struct AsyncVideoPlayer {
    stop_flag: Arc<AtomicBool>,
    _thread: Option<JoinHandle<()>>,
    frame_index: Arc<AtomicUsize>,
}

impl AsyncVideoPlayer {
    fn new(tx: Option<Sender<DaemonEvent>>, asset: Arc<MediaAsset>) -> Self {
        let stop_flag = Arc::new(AtomicBool::new(false));
        let stop_clone = Arc::clone(&stop_flag);

        let tx_for_thread = tx.clone();

        let asset_for_thread = Arc::clone(&asset);

        let min_dur = Duration::from_millis(10);
        let std_dur = Duration::from_millis(100);

        let frame_durations: Vec<Duration> = if let MediaAssetKind::Video {
            frame_durations, ..
        } = &asset.kind
        {
            frame_durations.iter().map(|&d| d.max(min_dur)).collect()
        } else {
            vec![min_dur; 1]
        };

        let frame_index: Arc<AtomicUsize> = Arc::new(0.into());
        let frame_index_cloned = frame_index.clone();

        let thread = thread::spawn(move || {
            while !stop_clone.load(Ordering::Relaxed) {
                let mut frame_cnt = 0;
                if let Some(ref tx) = tx_for_thread {
                    frame_cnt = frame_index.fetch_add(1, Ordering::SeqCst);
                    let event = DaemonEvent::FrameFinished {
                        asset: Arc::clone(&asset_for_thread),
                    };
                    if tx.send(event).is_err() {
                        break;
                    }
                }

                if stop_clone.load(Ordering::Relaxed) {
                    break;
                }

                let millis = frame_durations.get(frame_cnt % frame_durations.len());
                thread::sleep(*millis.unwrap_or(&std_dur));
            }
        });

        Self {
            stop_flag,
            _thread: Some(thread),
            frame_index: frame_index_cloned,
        }
    }

    fn get_frame_index(&self) -> usize {
        self.frame_index.load(Ordering::SeqCst)
    }
}

impl Drop for AsyncVideoPlayer {
    fn drop(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
    }
}

struct AsyncCustomRenderer {
    current_frame: Arc<Mutex<FrameInfo>>,
    stop_flag: Arc<AtomicBool>,
    _thread: Option<JoinHandle<()>>,
}

impl AsyncCustomRenderer {
    fn new(
        tx: Option<Sender<DaemonEvent>>,
        asset: Arc<CustomAsset>,
        baseasset: Arc<MediaAsset>,
    ) -> Self {
        let initial = match asset.render_frame(true) {
            Ok(Some(frame)) => frame,
            Ok(None) => asset.blank_frame(),
            Err(err) => {
                warn!("Custom initial render failed: {err}");
                asset.blank_frame()
            }
        };

        let current_frame = Arc::new(Mutex::new(initial));
        let stop_flag = Arc::new(AtomicBool::new(false));
        let update_interval = asset.update_interval();

        let asset_clone = Arc::clone(&asset);
        let frame_clone = Arc::clone(&current_frame);
        let stop_clone = Arc::clone(&stop_flag);

        let asset_for_thread = Arc::clone(&baseasset);
        let tx_for_thread = tx.clone();

        let thread = thread::spawn(move || {
            let mut next_deadline = Instant::now() + update_interval;
            while !stop_clone.load(Ordering::Relaxed) {
                let now = Instant::now();
                if now < next_deadline {
                    thread::sleep(next_deadline - now);
                }
                if stop_clone.load(Ordering::Relaxed) {
                    break;
                }
                next_deadline += update_interval;
                if next_deadline < Instant::now() {
                    next_deadline = Instant::now() + update_interval;
                }
                match asset_clone.render_frame(false) {
                    Ok(Some(new_frame)) => {
                        *frame_clone.lock() = new_frame;
                        if let Some(ref tx) = tx_for_thread {
                            let event = DaemonEvent::FrameFinished {
                                asset: Arc::clone(&asset_for_thread),
                            };
                            if tx.send(event).is_err() {
                                break;
                            }
                        }
                    }
                    Ok(None) => {}
                    Err(err) => {
                        warn!("Custom background render failed: {err}");
                    }
                }
            }
        });

        Self {
            current_frame,
            stop_flag,
            _thread: Some(thread),
        }
    }

    fn get_frame_index(&self) -> usize {
        self.current_frame.lock().frame_index
    }

    fn get_current_frame(&self) -> Vec<u8> {
        self.current_frame.lock().data.clone()
    }
}

impl Drop for AsyncCustomRenderer {
    fn drop(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
    }
}

impl MediaRuntime {
    fn from_asset(asset: Arc<MediaAsset>, tx: Option<Sender<DaemonEvent>>) -> Self {
        match &asset.kind {
            MediaAssetKind::Static { frame } => Self::Static {
                frame: Arc::clone(frame),
            },
            MediaAssetKind::Video { frames, .. } => {
                let player = Arc::new(AsyncVideoPlayer::new(tx, Arc::clone(&asset)));

                Self::Video {
                    player,
                    frames: Arc::clone(frames),
                    sent_frame_index: 0,
                }
            }

            MediaAssetKind::Sensor {
                asset: sensor_asset,
            } => {
                let renderer = Arc::new(AsyncSensorRenderer::new(
                    tx,
                    Arc::clone(sensor_asset),
                    Arc::clone(&asset),
                ));
                let cached_frame = renderer.get_current_frame();
                Self::Sensor {
                    renderer,
                    cached_frame,
                    sent_frame_index: 0,
                }
            }
            MediaAssetKind::H264Stream { path, looping, .. } => Self::H264 {
                path: path.clone(),
                looping: *looping,
                started: false,
            },
            MediaAssetKind::Custom {
                asset: custom_asset,
            } => {
                let renderer = Arc::new(AsyncCustomRenderer::new(
                    tx,
                    Arc::clone(custom_asset),
                    Arc::clone(&asset),
                ));

                let cached_frame = renderer.get_current_frame();
                Self::Custom {
                    renderer,
                    cached_frame,
                    sent_frame_index: 0,
                }
            }
        }
    }

    fn next_frame_bytes(&mut self) -> Option<&[u8]> {
        match self {
            MediaRuntime::Static { frame } => Some(frame.as_slice()),
            MediaRuntime::Video {
                player,
                frames,
                sent_frame_index,
                ..
            } => {
                let rendered_frame_index = player.get_frame_index();
                if rendered_frame_index <= *sent_frame_index || frames.is_empty() {
                    return None;
                }
                let ret = Some(frames[rendered_frame_index % frames.len()].as_slice());
                *sent_frame_index = rendered_frame_index;
                ret
            }
            MediaRuntime::Sensor {
                renderer,
                cached_frame,
                sent_frame_index,
                ..
            } => {
                let rendered_frame_index = renderer.get_frame_index();
                if rendered_frame_index <= *sent_frame_index {
                    return None;
                }
                *cached_frame = renderer.get_current_frame();
                *sent_frame_index = rendered_frame_index;
                Some(cached_frame.as_slice())
            }
            MediaRuntime::Custom {
                renderer,
                cached_frame,
                sent_frame_index,
                ..
            } => {
                let rendered_frame_index = renderer.get_frame_index();
                if rendered_frame_index <= *sent_frame_index {
                    return None;
                }
                *cached_frame = renderer.get_current_frame();
                *sent_frame_index = rendered_frame_index;
                Some(cached_frame.as_slice())
            }
            MediaRuntime::H264 { .. } => None,
        }
    }
}

pub(super) enum SendError {
    Usb(lianli_transport::TransportError),
    Other(anyhow::Error),
}

pub(super) fn parse_mac_str(s: &str) -> Option<[u8; 6]> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 6 {
        return None;
    }
    let mut mac = [0u8; 6];
    for (i, part) in parts.iter().enumerate() {
        mac[i] = u8::from_str_radix(part, 16).ok()?;
    }
    Some(mac)
}
