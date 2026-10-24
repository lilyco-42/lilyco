use lilyco::prelude::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const MAX_INPUT_BYTES: u64 = 25 * 1024 * 1024;
const MAX_PIXELS: u64 = 50_000_000;
const TIMEOUT: Duration = Duration::from_secs(300);

/// Model-based local background removal exposed directly to Lilyco agents via MCP.
/// Requires locally installed trusted rembg CLI. No Lilyco server inference.
#[derive(App)]
#[app(
    name = "remove-background",
    run = "run_remove_background",
    about = "Remove the background of a local image using a CLIENT-SIDE ONNX rembg model. Returns a new transparent PNG path for downstream AI tools. Remains on the user's machine; requires rembg[cpu,cli] or rembg[gpu,cli] installed locally. Select u2netp, u2net, silueta, or isnet-general-use. Input <=25MiB/50MP; never overwrite files. First use may download the model to the client's cache."
)]
pub(crate) struct RemoveBackground {
    #[arg(about = "Existing local PNG/JPEG/WebP input image", must_exist = true)]
    pub(crate) image: PathBuf,

    #[arg(about = "New output path with .png extension; default: input.nobg.png")]
    pub(crate) out: Option<String>,

    #[arg(
        default = "u2netp",
        about = "Allowed local model: u2netp, u2net, silueta, isnet-general-use"
    )]
    pub(crate) model: String,
}

fn check_model(model: &str) -> Result<&str, AppError> {
    match model {
        "u2netp" | "u2net" | "silueta" | "isnet-general-use" => Ok(model),
        _ => Err(AppError::InvalidArg(
            "model must be u2netp, u2net, silueta, or isnet-general-use".into(),
        )),
    }
}

fn rembg_binary() -> Result<PathBuf, AppError> {
    if let Some(path) = std::env::var_os("LILYCO_REMBG_BIN") {
        let path = PathBuf::from(path);
        if !path.is_absolute() || !path.is_file() {
            return Err(AppError::InvalidArg(
                "LILYCO_REMBG_BIN must be an absolute path to a local rembg executable".into(),
            ));
        }
        return Ok(path);
    }
    let name = if cfg!(windows) { "rembg.exe" } else { "rembg" };
    if let Some(env_path) = std::env::var_os("PATH") {
        for folder in std::env::split_paths(&env_path) {
            let path = folder.join(name);
            if path.is_file() {
                return Ok(path);
            }
        }
    }
    Err(AppError::Runtime(
        "rembg CLI not installed on this client. Install rembg[cpu,cli] or rembg[gpu,cli], or configure LILYCO_REMBG_BIN.".into(),
    ))
}

fn paths(app: &RemoveBackground) -> Result<(PathBuf, PathBuf), AppError> {
    check_model(&app.model)?;
    let input = fs::canonicalize(&app.image)?;
    if !input.is_file() {
        return Err(AppError::InvalidInput("input must be a file".into()));
    }
    let len = fs::metadata(&input)?.len();
    if len == 0 || len > MAX_INPUT_BYTES {
        return Err(AppError::InvalidInput(
            "input must be 1 byte to 25 MiB".into(),
        ));
    }
    let reader = image::ImageReader::open(&input)
        .map_err(|e| AppError::InvalidInput(e.to_string()))?
        .with_guessed_format()
        .map_err(|e| AppError::InvalidInput(e.to_string()))?;
    if !matches!(
        reader.format(),
        Some(image::ImageFormat::Png | image::ImageFormat::Jpeg | image::ImageFormat::WebP)
    ) {
        return Err(AppError::InvalidInput(
            "input must be PNG, JPEG, or WebP".into(),
        ));
    }
    let (w, h) = image::image_dimensions(&input)
        .map_err(|e| AppError::InvalidInput(format!("image dimensions: {e}")))?;
    if w == 0 || h == 0 || u64::from(w) * u64::from(h) > MAX_PIXELS {
        return Err(AppError::InvalidInput(
            "image exceeds 50 million pixels".into(),
        ));
    }
    let stem = input
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("image");
    let output = app
        .out
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(|| input.with_file_name(format!("{stem}.nobg.png")));
    if output
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.eq_ignore_ascii_case("png"))
        != Some(true)
    {
        return Err(AppError::InvalidArg("output must be .png".into()));
    }
    if output.exists() {
        return Err(AppError::Safety(format!(
            "output exists; refusing overwrite: {}",
            output.display()
        )));
    }
    let parent = output
        .parent()
        .filter(|x| !x.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent = fs::canonicalize(parent)?;
    if !parent.is_dir() {
        return Err(AppError::InvalidArg("output directory missing".into()));
    }
    let filename = output
        .file_name()
        .ok_or_else(|| AppError::InvalidArg("bad output filename".into()))?;
    let output = parent.join(filename);
    if output == input {
        return Err(AppError::Safety("output equals input".into()));
    }
    Ok((input, output))
}

fn cli_args(model: &str, input: &Path, output: &Path) -> Vec<std::ffi::OsString> {
    vec![
        "i".into(),
        "-m".into(),
        model.into(),
        input.as_os_str().to_os_string(),
        output.as_os_str().to_os_string(),
    ]
}

pub(crate) fn run_remove_background(
    app: &RemoveBackground,
    ctx: &Context,
) -> Result<serde_json::Value, AppError> {
    let start = Instant::now();
    let (input, out) = paths(app)?;
    let bin = rembg_binary()?;
    let parent = out
        .parent()
        .ok_or_else(|| AppError::InvalidArg("invalid output".into()))?;
    let temp = tempfile::Builder::new()
        .prefix(".lilyco-rembg-")
        .suffix(".png")
        .tempfile_in(parent)?;

    ctx.emit(Progress::Started {
        total: None,
        message: Some(format!("local AI background removal: {}", app.model)),
    });
    let mut child = Command::new(bin)
        .args(cli_args(check_model(&app.model)?, &input, temp.path()))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| AppError::Runtime(format!("starting local rembg: {e}")))?;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(Some(status)) => {
                return Err(AppError::Runtime(format!("local rembg failed: {status}")))
            }
            Ok(None) => (),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(AppError::Runtime(format!("waiting for rembg: {e}")));
            }
        }
        let cancelled = ctx.is_cancelled();
        if cancelled || start.elapsed() >= TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            return if cancelled {
                Err(AppError::Cancelled)
            } else {
                Err(AppError::Runtime("local rembg timed out after 300s".into()))
            };
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let bytes = fs::metadata(temp.path())?.len();
    if bytes == 0 || bytes > 200 * 1024 * 1024 {
        return Err(AppError::Runtime("empty or oversized output".into()));
    }
    let reader = image::ImageReader::open(temp.path())
        .map_err(|e| AppError::Runtime(e.to_string()))?
        .with_guessed_format()
        .map_err(|e| AppError::Runtime(e.to_string()))?;
    if reader.format() != Some(image::ImageFormat::Png) {
        return Err(AppError::Runtime("rembg output was not a PNG".into()));
    }
    let (width, height) =
        image::image_dimensions(temp.path()).map_err(|e| AppError::Runtime(e.to_string()))?;
    temp.persist_noclobber(&out)
        .map_err(|e| AppError::Runtime(format!("saving output (refuse overwrite): {}", e.error)))?;
    let result = serde_json::json!({
        "ok": true, "input": input.to_string_lossy(), "out": out.to_string_lossy(),
        "model": app.model, "backend": "rembg-local",
        "width": width, "height": height, "bytes": bytes,
        "duration_ms": start.elapsed().as_millis() as u64,
    });
    ctx.done(result.clone(), start.elapsed().as_millis() as u64);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_local_models_allowed() {
        for model in ["u2netp", "u2net", "silueta", "isnet-general-use"] {
            assert!(check_model(model).is_ok());
        }
        for model in ["withoutbg", "u2net_custom", "https://api.example", "-x"] {
            assert!(check_model(model).is_err());
        }
    }
    #[test]
    fn refuses_non_images_and_overwriting() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("picture.png");
        image::RgbaImage::from_pixel(8, 8, image::Rgba([255, 0, 0, 255]))
            .save(&input)
            .unwrap();
        let mut app = RemoveBackground {
            image: input,
            out: None,
            model: "u2netp".into(),
        };
        assert!(paths(&app).is_ok());
        let output = dir.path().join("output.png");
        app.out = Some(output.to_string_lossy().to_string());
        assert!(paths(&app).is_ok());
        fs::write(output, b"existing").unwrap();
        assert!(paths(&app).is_err());
    }
    #[test]
    fn fixed_cli_arguments() {
        let args = cli_args("u2netp", Path::new("input.jpg"), Path::new("out.png"));
        let strings: Vec<_> = args
            .iter()
            .map(|x| x.to_string_lossy().to_string())
            .collect();
        assert_eq!(strings, ["i", "-m", "u2netp", "input.jpg", "out.png"]);
    }
}
