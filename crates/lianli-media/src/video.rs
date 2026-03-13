use super::common::{apply_orientation, encode_jpeg, MediaError};
use image::codecs::gif::GifDecoder;
use image::imageops::FilterType;
use image::{load_from_memory, AnimationDecoder, DynamicImage};
use lianli_shared::screen::ScreenInfo;
use std::fs::File;
use std::path::Path;
use std::process::Command;
use std::time::Duration;
use tempfile::TempDir;

pub fn build_video_frames(
    path: &Path,
    fps: f32,
    orientation: f32,
    screen: &ScreenInfo,
) -> Result<(Vec<Vec<u8>>, Vec<Duration>), MediaError> {
    let temp = TempDir::new()?;
    let output_pattern = temp.path().join("frame_%05d.jpg");
    run_ffmpeg(path, fps, &output_pattern, screen)?;

    let mut entries: Vec<_> = std::fs::read_dir(temp.path())?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|p| p.extension().map(|ext| ext == "jpg").unwrap_or(false))
        .collect();
    entries.sort();

    if entries.is_empty() {
        return Err(MediaError::EmptyVideo);
    }

    let mut frames = Vec::with_capacity(entries.len());
    for frame_path in entries {
        let data = std::fs::read(&frame_path)?;
        if orientation.abs() < f32::EPSILON && screen.device_rotation == 0 {
            if data.len() > screen.max_payload {
                return Err(MediaError::PayloadTooLarge { size: data.len() });
            }
            frames.push(data);
        } else {
            let image = load_from_memory(&data)?;
            let rgb = apply_orientation(image.to_rgb8(), orientation);
            frames.push(encode_jpeg(rgb, screen)?);
        }
    }

    let interval = Duration::from_secs_f32(1.0 / fps);
    let durations = vec![interval; frames.len()];
    Ok((frames, durations))
}

pub fn build_gif_frames(
    path: &Path,
    orientation: f32,
    screen: &ScreenInfo,
) -> Result<(Vec<Vec<u8>>, Vec<Duration>), MediaError> {
    let file = File::open(path)?;
    let decoder = GifDecoder::new(file)?;
    let frames = decoder.into_frames();
    let mut encoded = Vec::new();
    let mut durations = Vec::new();

    for frame in frames {
        let frame = frame?;
        let (numer, denom) = frame.delay().numer_denom_ms();
        let millis = if denom == 0 {
            numer as f32
        } else {
            numer as f32 / denom as f32
        };
        let duration = Duration::from_millis(millis.max(10.0) as u64);
        let rgba = frame.into_buffer();
        let rgb = DynamicImage::ImageRgba8(rgba).to_rgb8();
        let resized =
            image::imageops::resize(&rgb, screen.width, screen.height, FilterType::Lanczos3);
        let oriented = apply_orientation(resized, orientation);
        let jpeg = encode_jpeg(oriented, screen)?;
        encoded.push(jpeg);
        durations.push(duration);
    }

    if encoded.is_empty() {
        return Err(MediaError::EmptyVideo);
    }

    Ok((encoded, durations))
}

fn run_ffmpeg(
    input: &Path,
    fps: f32,
    output_pattern: &Path,
    screen: &ScreenInfo,
) -> Result<(), MediaError> {
    let scale_filter = format!(
        "scale={}:{}:flags=lanczos",
        screen.width, screen.height
    );
    let status = Command::new("ffmpeg")
        .args([
            "-y",
            "-loglevel",
            "error",
            "-i",
            input.to_str().unwrap(),
            "-vf",
            &scale_filter,
            "-r",
            &fps.to_string(),
            "-q:v",
            "4",
            output_pattern.to_str().unwrap(),
        ])
        .status()
        .map_err(MediaError::Io)?;

    if !status.success() {
        return Err(MediaError::Ffmpeg(format!(
            "ffmpeg exited with status {status}"
        )));
    }

    Ok(())
}
