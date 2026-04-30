use crate::common::MediaError;
use std::path::Path;
use std::process::Command;

pub(super) fn run_ffmpeg(
    input: &Path,
    fps: f32,
    output_pattern: &Path,
    width: u32,
    height: u32,
) -> Result<(), MediaError> {
    let scale_filter = format!("scale={width}:{height}:flags=lanczos");
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

pub(super) fn run_ffmpeg_rgba(
    input: &Path,
    fps: f32,
    output_pattern: &Path,
    width: u32,
    height: u32,
) -> Result<(), MediaError> {
    let scale_filter = format!("scale={width}:{height}:flags=lanczos");
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
            "-pix_fmt",
            "rgba",
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
