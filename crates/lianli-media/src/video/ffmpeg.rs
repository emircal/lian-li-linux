use crate::common::MediaError;
use std::path::Path;
use std::process::Command;

fn hwaccel_args() -> Vec<String> {
    if std::env::var("LIANLI_DISABLE_HW_VIDEO")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
    {
        Vec::new()
    } else {
        vec!["-hwaccel".into(), "auto".into()]
    }
}

pub(super) fn run_ffmpeg(
    input: &Path,
    fps: f32,
    output_pattern: &Path,
    width: u32,
    height: u32,
) -> Result<(), MediaError> {
    let scale_filter = format!("scale={width}:{height}:flags=lanczos");
    let mut args: Vec<String> = vec!["-y".into(), "-loglevel".into(), "error".into()];
    args.extend(hwaccel_args());
    args.extend([
        "-i".into(),
        input.to_str().unwrap().into(),
        "-vf".into(),
        scale_filter,
        "-r".into(),
        fps.to_string(),
        "-q:v".into(),
        "4".into(),
        output_pattern.to_str().unwrap().into(),
    ]);
    let output = Command::new("ffmpeg")
        .args(&args)
        .output()
        .map_err(MediaError::Io)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(MediaError::Ffmpeg(format!(
            "ffmpeg exited with status {}: {}",
            output.status,
            stderr.trim()
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
    let mut args: Vec<String> = vec!["-y".into(), "-loglevel".into(), "error".into()];
    args.extend(hwaccel_args());
    args.extend([
        "-i".into(),
        input.to_str().unwrap().into(),
        "-vf".into(),
        scale_filter,
        "-r".into(),
        fps.to_string(),
        "-pix_fmt".into(),
        "rgba".into(),
        output_pattern.to_str().unwrap().into(),
    ]);
    let output = Command::new("ffmpeg")
        .args(&args)
        .output()
        .map_err(MediaError::Io)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(MediaError::Ffmpeg(format!(
            "ffmpeg exited with status {}: {}",
            output.status,
            stderr.trim()
        )));
    }

    Ok(())
}
