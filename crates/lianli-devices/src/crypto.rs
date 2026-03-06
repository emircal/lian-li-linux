use cbc::Encryptor;
use des::cipher::{block_padding::Pkcs7, BlockEncryptMut, KeyIvInit};
use des::Des;
use std::time::{SystemTime, UNIX_EPOCH};

const DES_KEY: [u8; 8] = *b"slv3tuzx";

type DesCbc = Encryptor<Des>;

// Command types for VID=0x1CBE LCD devices.
pub const CMD_ROTATE: u8 = 0x0D;
pub const CMD_BRIGHTNESS: u8 = 0x0E;
pub const CMD_FRAME_RATE: u8 = 0x0F;
pub const CMD_PUSH_JPG: u8 = 0x65;
pub const CMD_START_PLAY: u8 = 0x79;
pub const CMD_QUERY_BLOCK: u8 = 0x7A;
pub const CMD_STOP_PLAY: u8 = 0x7B;

/// Builds DES-CBC encrypted command headers for VID=0x1CBE LCD devices.
///
/// All VID=0x1CBE devices (SLV3, TLV2, HydroShift II, Lancool 207, Universal Screen)
/// share this exact same encrypted 512-byte header format.
pub struct PacketBuilder {
    last_timestamp: u32,
}

impl PacketBuilder {
    pub fn new() -> Self {
        Self { last_timestamp: 0 }
    }

    /// Build a 512-byte encrypted header with raw parameter bytes at offset 8+.
    fn build(&mut self, command: u8, params: &[u8]) -> Vec<u8> {
        let mut buf = vec![0u8; 504 + 8];
        buf[0] = command;
        buf[2] = 0x1A;
        buf[3] = 0x6D;

        let raw = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as u32;
        let ts = if raw <= self.last_timestamp {
            self.last_timestamp + 1
        } else {
            raw
        };
        self.last_timestamp = ts;
        buf[4..8].copy_from_slice(&ts.to_le_bytes());

        let copy_len = params.len().min(496);
        buf[8..8 + copy_len].copy_from_slice(&params[..copy_len]);

        let cipher = DesCbc::new_from_slices(&DES_KEY, &DES_KEY)
            .expect("DES key and IV must both be 8 bytes");
        cipher
            .encrypt_padded_mut::<Pkcs7>(&mut buf, 504)
            .expect("padding")
            .to_vec()
    }

    /// Build a 512-byte encrypted header.
    ///
    /// - `payload_size`: size of the JPEG payload that follows
    /// - `command`: command byte (e.g., CMD_PUSH_JPG)
    /// - `include_size`: whether to embed the payload size at offset 8
    pub fn header(&mut self, payload_size: usize, command: u8, include_size: bool) -> Vec<u8> {
        if include_size {
            self.build(command, &(payload_size as u32).to_be_bytes())
        } else {
            self.build(command, &[])
        }
    }

    /// Build a JPEG frame header (cmd 0x65 with payload size).
    pub fn jpeg_header(&mut self, jpeg_size: usize) -> Vec<u8> {
        self.header(jpeg_size, CMD_PUSH_JPG, true)
    }

    /// Build a brightness control header (cmd 0x0E, value 0-100).
    pub fn brightness_header(&mut self, brightness: u8) -> Vec<u8> {
        self.build(CMD_BRIGHTNESS, &[brightness.min(100)])
    }

    /// Build a rotation control header (cmd 0x0D, value 0-3).
    pub fn rotation_header(&mut self, rotation: u8) -> Vec<u8> {
        self.build(CMD_ROTATE, &[rotation & 0x03])
    }

    /// Build a frame rate control header (cmd 0x0F).
    pub fn frame_rate_header(&mut self, fps: u8) -> Vec<u8> {
        self.build(CMD_FRAME_RATE, &[fps])
    }

    // ── H2 (HydroShift II) packet format ─────────────────────────────────────
    //
    // WinUsbH2.cs uses a 500-byte plaintext (GetBaseCmdBuf), DES-CBC-PKCS7
    // encrypts it to 504 bytes, then places the result in a 512-byte frame with
    // fixed trailer bytes [510]=0xa1, [511]=0x1a.  This differs from the SLV3
    // format (504-byte plaintext is 512 encrypted, no trailer).

    fn build_h2(&mut self, command: u8, params: &[u8]) -> Vec<u8> {
        // 500-byte plaintext; need 500 + block_size(8) bytes for encrypt_padded_mut
        let mut buf = vec![0u8; 508];
        buf[0] = command;
        buf[2] = 0x1A;
        buf[3] = 0x6D;

        let raw = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as u32;
        let ts = if raw <= self.last_timestamp {
            self.last_timestamp + 1
        } else {
            raw
        };
        self.last_timestamp = ts;
        buf[4..8].copy_from_slice(&ts.to_le_bytes());

        let copy_len = params.len().min(492);
        buf[8..8 + copy_len].copy_from_slice(&params[..copy_len]);

        // Encrypt the first 500 bytes: PKCS7 pads 500 -> 504 bytes (adds 4 bytes)
        let cipher = DesCbc::new_from_slices(&DES_KEY, &DES_KEY)
            .expect("DES key and IV must both be 8 bytes");
        let encrypted = cipher
            .encrypt_padded_mut::<Pkcs7>(&mut buf, 500)
            .expect("padding")
            .to_vec();
        // encrypted.len() == 504

        // Build the 512-byte header: encrypted + zeros + trailer
        let mut out = vec![0u8; 512];
        out[..504].copy_from_slice(&encrypted);
        out[510] = 0xa1;
        out[511] = 0x1a;
        out
    }

    /// Build an H2 JPEG frame header (cmd 0x65 with payload size).
    pub fn jpeg_header_h2(&mut self, jpeg_size: usize) -> Vec<u8> {
        self.build_h2(CMD_PUSH_JPG, &(jpeg_size as u32).to_be_bytes())
    }

    /// Build an H2 frame rate header (cmd 0x0F).
    pub fn frame_rate_header_h2(&mut self, fps: u8) -> Vec<u8> {
        self.build_h2(CMD_FRAME_RATE, &[fps])
    }

    /// Build an H2 rotation header (cmd 0x0D, value 0-3).
    pub fn rotation_header_h2(&mut self, rotation: u8) -> Vec<u8> {
        self.build_h2(CMD_ROTATE, &[rotation & 0x03])
    }

    /// Build an H2 brightness header (cmd 0x0E, value 0-100).
    pub fn brightness_header_h2(&mut self, brightness: u8) -> Vec<u8> {
        self.build_h2(CMD_BRIGHTNESS, &[brightness.min(100)])
    }
}

impl Default for PacketBuilder {
    fn default() -> Self {
        Self::new()
    }
}
