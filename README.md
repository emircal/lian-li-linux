<p align="center">
  <img src="crates/lianli-gui/src-tauri/icons/icon.svg" width="128" height="128" alt="Lian Li Linux">
</p>

<h1 align="center">Lian Li Linux</h1>

<p align="center">
  Open-source Linux replacement for L-Connect 3.<br>
  Fan speed control, RGB/LED effects, LCD streaming, and sensor gauges for all Lian Li devices.
</p>

---

## Supported Devices

### Wired (HID)

| Device | Fan Control | RGB | LCD | Pump |
|--------|:-----------:|:---:|:---:|:----:|
| UNI FAN SL / AL / SL Infinity / SL V2 / AL V2 (ENE 6K77) | 4 groups | Yes | - | - |
| UNI FAN TL Controller | 4 ports | Yes | - | - |
| UNI FAN TL LCD | 4 ports | Yes | 400x400 | - |
| Galahad II Trinity AIO | Yes | Yes | - | Yes |
| HydroShift LCD AIO | Yes | Yes | 480x480 | Yes |
| Galahad II LCD / Vision AIO | Yes | Yes | 480x480 | Yes |

### Wireless (USB Bulk via TX/RX dongle)

| Device | RGB | LCD | Notes |
|--------|:---:|:---:|-------|
| UNI FAN SL V3 (LCD / LED) | Yes | 480x480 | 120mm / 140mm |
| UNI FAN TL V2 (LCD / LED) | Yes | 480x480 | 120mm / 140mm |
| UNI FAN SL-INF | Yes | - | Wireless |
| UNI FAN CL / RL120 | Yes | - | Wireless |
| HydroShift II LCD Circle | - | 480x480 | WinUSB |
| Lancool 207 Digital | - | 1472x720 | WinUSB |
| Universal Screen 8.8" | - | 1920x480 | WinUSB |

## Architecture

```
lianli-daemon          User service - fan control loop + LCD streaming
  lianli-devices       HID/USB device drivers
  lianli-transport     USB bulk transport (wireless protocol, display streaming)
  lianli-media         Image/video/GIF encoding, sensor gauge rendering
  lianli-shared        IPC types, config schema, device IDs

lianli-gui             Tauri 2 + Vue 3 desktop app - connects to daemon via Unix socket
```

The daemon runs as a user systemd service. USB access is granted via udev rules (no root required).
The GUI connects over `$XDG_RUNTIME_DIR/lianli-daemon.sock`.

## Prerequisites

- **Rust** (stable, 1.75+)
- **Bun** (for the GUI frontend)
- **ffmpeg** and **ffprobe** in `PATH` (for video/GIF decoding)
- **System libraries:**

```bash
# Arch
sudo pacman -S hidapi libusb webkit2gtk gtk3 librsvg ffmpeg

# Ubuntu / Debian
sudo apt install libhidapi-dev libusb-1.0-0-dev libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev ffmpeg

# Fedora
sudo dnf install hidapi-devel libusb1-devel webkit2gtk4.1-devel gtk3-devel librsvg2-devel ffmpeg
```

## Building

```bash
git clone --recurse-submodules https://github.com/sgtaziz/LIAN-LI-LINUX.git
cd LIAN-LI-LINUX

# If you already cloned without --recurse-submodules:
git submodule update --init --recursive

# Install GUI frontend dependencies and build everything
cd crates/lianli-gui && bun install && cd ../..
cargo build --release
```

Binaries: `target/release/lianli-daemon` and `target/release/lianli-gui`

## Installation

### 1. udev rules

Required so the daemon can access USB devices without root:

```bash
sudo cp udev/99-lianli.rules /etc/udev/rules.d/
sudo udevadm control --reload-rules
sudo udevadm trigger
```

### 2. Daemon

```bash
# Copy binary
cp target/release/lianli-daemon ~/.local/bin/

# Install and start user systemd service
mkdir -p ~/.config/systemd/user
cp systemd/lianli-daemon.service ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now lianli-daemon
```

A default config is created automatically at `~/.config/lianli/config.json` on first run.

### 3. GUI

```bash
cp target/release/lianli-gui ~/.local/bin/

# Install icons
for size in 32x32 128x128 256x256; do mkdir -p ~/.local/share/icons/hicolor/$size/apps; done
cp crates/lianli-gui/src-tauri/icons/32x32.png ~/.local/share/icons/hicolor/32x32/apps/lianli-gui.png
cp crates/lianli-gui/src-tauri/icons/128x128.png ~/.local/share/icons/hicolor/128x128/apps/lianli-gui.png
cp crates/lianli-gui/src-tauri/icons/128x128@2x.png ~/.local/share/icons/hicolor/256x256/apps/lianli-gui.png

# Install desktop entry
cp lianli-gui.desktop ~/.local/share/applications/
update-desktop-database ~/.local/share/applications/
```

## Configuration

The daemon reads `~/.config/lianli/config.json`. The GUI edits this file via the daemon's IPC socket.

### LCD Streaming

Each LCD entry specifies a target device (by serial), media type, and orientation:

| Type | Description |
|------|-------------|
| `image` | Static image (JPEG, PNG, BMP, GIF) |
| `video` | Video file (decoded frame-by-frame via ffmpeg) |
| `gif` | Animated GIF |
| `color` | Solid RGB color |
| `sensor` | Live sensor gauge (CPU temp, GPU temp, etc.) |

### Fan Curves

Fan curves map a temperature source (any shell command) to a speed percentage.
Points are linearly interpolated; temperatures outside the curve range clamp to the nearest point's speed.

### Fan Speed Modes

| Mode | Description |
|------|-------------|
| `0` | Off (0% PWM) |
| `"curve-name"` | Follow a named fan curve |
| `1-255` | Constant PWM duty (1=0.4%, 128=50%, 255=100%) |
| `"__mb_sync__"` | Mirror motherboard PWM signal (hardware passthrough) |

## Troubleshooting

**Daemon won't start / no devices found:**
```bash
# Check udev rules are loaded
sudo udevadm test /sys/bus/usb/devices/<your-device>

# Check daemon logs
journalctl --user -u lianli-daemon -f
```

**GUI says "Daemon offline":**
```bash
# Verify daemon is running
systemctl --user status lianli-daemon

# Check socket exists
ls -la $XDG_RUNTIME_DIR/lianli-daemon.sock
```

**Permission denied on USB device:**
```bash
# Re-trigger udev after plugging in device
sudo udevadm trigger
```

## License

MIT. See [LICENSE](LICENSE).

This project is not affiliated with Lian Li Industrial Co., Ltd.
Protocol information was obtained through reverse engineering for interoperability purposes.
