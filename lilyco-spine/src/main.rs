//! lspine — inspect Spine JSON skeletons and atlas metadata, without a proprietary runtime.
//! This is *not* the Spine editor or viewer. Parsing metadata does not entail animation rendering.
use lilyco::prelude::*;
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;
use std::time::Instant;

const MAX_BYTES: u64 = 32 * 1024 * 1024;
const MAX_ITEMS: usize = 256;

#[derive(App)]
#[app(
    name = "inspect-skeleton",
    run = "inspect_skeleton",
    about = "Inspect local Spine JSON skeleton metadata: version, bones, slots, skins, constraints, animation names. No rendering, no runtime licensing implied. Input is local JSON <=32MiB; never modifies files."
)]
struct InspectSkeleton {
    #[arg(
        about = "Existing local Spine JSON skeleton (.json, .json.bytes)",
        must_exist = true
    )]
    file: PathBuf,
}

#[derive(App)]
#[app(
    name = "inspect-atlas",
    run = "inspect_atlas",
    about = "Read a local plain-text Spine texture atlas (.atlas or .atlas.txt), report pages and named regions, and identify missing local texture pages. No renderer required. Never modifies files."
)]
struct InspectAtlas {
    #[arg(
        about = "Existing local Spine atlas file (.atlas/.atlas.txt)",
        must_exist = true
    )]
    file: PathBuf,
}

fn read_checked(file: &PathBuf, ext: &[&str]) -> Result<(PathBuf, Vec<u8>), AppError> {
    let path = fs::canonicalize(file)?;
    if !path.is_file() {
        return Err(AppError::InvalidInput("file must be regular file".into()));
    }
    let name = path
        .file_name()
        .and_then(|x| x.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if !ext.iter().any(|suffix| name.ends_with(suffix)) {
        return Err(AppError::InvalidInput(format!(
            "file must have extension: {}",
            ext.join(", ")
        )));
    }
    let size = fs::metadata(&path)?.len();
    if size == 0 || size > MAX_BYTES {
        return Err(AppError::InvalidInput(
            "file must be 1 byte to 32 MiB".into(),
        ));
    }
    let data = fs::read(&path)?;
    Ok((path, data))
}

fn names(value: Option<&Value>) -> Vec<String> {
    let mut result = match value {
        Some(Value::Object(map)) => map.keys().cloned().collect(),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| item.get("name").and_then(Value::as_str).map(str::to_owned))
            .collect(),
        _ => Vec::new(),
    };
    result.truncate(MAX_ITEMS);
    result
}

fn count(value: Option<&Value>) -> usize {
    match value {
        Some(Value::Array(items)) => items.len(),
        Some(Value::Object(map)) => map.len(),
        _ => 0,
    }
}

fn skeleton_metadata(bytes: &[u8]) -> Result<Value, AppError> {
    let v: Value = serde_json::from_slice(bytes)
        .map_err(|e| AppError::InvalidInput(format!("invalid Spine JSON: {e}")))?;
    let root = v
        .as_object()
        .ok_or_else(|| AppError::InvalidInput("Spine JSON must be an object".into()))?;
    if !root.contains_key("bones") || !v["bones"].is_array() {
        return Err(AppError::InvalidInput(
            "not a Spine JSON skeleton (bones array missing)".into(),
        ));
    }
    let skeleton = v.get("skeleton");
    let version = skeleton
        .and_then(|s| s.get("spine"))
        .and_then(Value::as_str);
    let result = json!({
        "ok":true, "kind":"spine-json-metadata",
        "version":version,
        "bones":count(v.get("bones")),
        "slots":count(v.get("slots")),
        "skins":count(v.get("skins")),
        "skin_names":names(v.get("skins")),
        "animation_count":count(v.get("animations")),
        "animation_names":names(v.get("animations")),
        "ik_constraints":count(v.get("ik")),
        "transform_constraints":count(v.get("transform")),
        "path_constraints":count(v.get("path")),
        "events":count(v.get("events")),
        "has_images_path":skeleton.and_then(|s|s.get("images")).is_some(),
        "truncated_name_lists": count(v.get("animations"))>MAX_ITEMS||count(v.get("skins"))>MAX_ITEMS
    });
    Ok(result)
}

fn inspect_skeleton(app: &InspectSkeleton, ctx: &Context) -> Result<Value, AppError> {
    let start = Instant::now();
    let (file, bytes) = read_checked(&app.file, &[".json", ".json.bytes"])?;
    ctx.emit(Progress::Started {
        total: None,
        message: Some(format!("inspect Spine {}", file.display())),
    });
    let mut value = skeleton_metadata(&bytes)?;
    value["file"] = json!(file.to_string_lossy());
    value["file_size_bytes"] = json!(bytes.len());
    ctx.done(value.clone(), start.elapsed().as_millis() as u64);
    Ok(value)
}

fn atlas_metadata(text: &str) -> Value {
    let mut pages = Vec::<String>::new();
    let mut regions = Vec::<String>::new();
    // Spine atlas format: each page starts with a filename following a blank
    // line; metadata lines contain ':' and regions follow the page metadata.
    let mut blank = true;
    let mut page_metadata = false;
    for line in text.lines() {
        let clean = line.trim();
        if clean.is_empty() {
            blank = true;
            page_metadata = false;
            continue;
        }
        if clean.starts_with('#') {
            continue;
        }
        if clean.contains(':') {
            blank = false;
            continue;
        }
        if line.starts_with(' ') || line.starts_with('\t') {
            blank = false;
            continue;
        }
        if pages.is_empty() || blank {
            pages.push(clean.to_owned());
            page_metadata = true;
        } else if !page_metadata {
            regions.push(clean.to_owned());
        } else {
            // non-property lines after page metadata are region names
            regions.push(clean.to_owned());
            page_metadata = false;
        }
        blank = false;
    }
    let page_count = pages.len();
    let region_count = regions.len();
    pages.truncate(MAX_ITEMS);
    regions.truncate(MAX_ITEMS);
    json!({
        "ok":true,"kind":"spine-atlas-metadata",
        "page_count":page_count, "region_count":region_count,
        "page_names":pages,"region_names":regions,
        "truncated_name_lists":page_count>MAX_ITEMS||region_count>MAX_ITEMS
    })
}

fn inspect_atlas(app: &InspectAtlas, ctx: &Context) -> Result<Value, AppError> {
    let start = Instant::now();
    let (file, bytes) = read_checked(&app.file, &[".atlas", ".atlas.txt"])?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| AppError::InvalidInput("atlas must be UTF-8 plain text".into()))?;
    ctx.emit(Progress::Started {
        total: None,
        message: Some(format!("inspect atlas {}", file.display())),
    });
    let mut value = atlas_metadata(text);
    let mut missing = Vec::new();
    if let Some(parent) = file.parent() {
        for page in value["page_names"].as_array().into_iter().flatten() {
            let name = page.as_str().unwrap_or_default();
            if name.contains('/') || name.contains('\\') || name == "." || name == ".." {
                missing.push(name.to_string());
                continue;
            }
            if !parent.join(name).is_file() {
                missing.push(name.to_owned())
            }
        }
    }
    value["missing_texture_pages"] = json!(missing);
    value["file"] = json!(file.to_string_lossy());
    ctx.done(value.clone(), start.elapsed().as_millis() as u64);
    Ok(value)
}

fn registry() -> Registry {
    let mut registry = Registry::new();
    registry
        .register(RegisteredCommand::from_app::<InspectSkeleton>())
        .expect("register inspect-skeleton");
    registry
        .register(RegisteredCommand::from_app::<InspectAtlas>())
        .expect("register inspect-atlas");
    registry
}

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.iter().any(|x| x == "--mcp") {
        lilyco::serve_mcp(registry());
        return;
    }
    if args.iter().any(|x| x == "--list") {
        println!("{}", registry().to_json());
        return;
    }
    eprintln!("usage: lspine --mcp | --list");
    std::process::exit(2);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn skeleton_metadata_supports_spine41_and_skins_object() {
        let bytes=br#"{"skeleton":{"spine":"4.1.20"},"bones":[{"name":"root"}],"slots":[{"name":"body"}],"skins":[{"name":"default"}],"animations":{"idle":{},"walk":{}}}"#;
        let result = skeleton_metadata(bytes).unwrap();
        assert_eq!(result["version"], "4.1.20");
        assert_eq!(result["bones"], 1);
        assert_eq!(result["skins"], 1);
        assert_eq!(result["animation_count"], 2);
        assert_eq!(result["animation_names"], json!(["idle", "walk"]));
    }
    #[test]
    fn rejects_arbitrary_json() {
        assert!(skeleton_metadata(br#"{"hello":"world"}"#).is_err());
        assert!(skeleton_metadata(b"not json").is_err());
    }
    #[test]
    fn atlas_finds_texture_pages_and_regions() {
        let input="page.png\nsize: 1024,1024\nfilter: Linear,Linear\nrepeat: none\nhead\n  bounds: 0,0,20,20\nbody\n  bounds: 20,20,30,30\n\npage2.png\nsize: 512,512\nhand\n  bounds: 1,1,9,9";
        let result = atlas_metadata(input);
        assert_eq!(result["page_count"], 2);
        assert_eq!(result["region_count"], 3);
    }
    #[test]
    fn registry_has_two_named_tools() {
        let names = registry().to_json().to_string();
        assert!(names.contains("inspect-skeleton"));
        assert!(names.contains("inspect-atlas"));
    }
    #[test]
    fn no_unbounded_read() {
        let dir = tempfile::tempdir().unwrap();
        let wrong = dir.path().join("secret.txt");
        fs::write(&wrong, b"hello").unwrap();
        assert!(read_checked(&wrong, &[".json"]).is_err());
    }
}
