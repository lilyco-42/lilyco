use std::{env, error::Error, fs, io, path::PathBuf};

fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-changed=resources");
    let directory = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?).join("resources");
    let mut files = if directory.is_dir() {
        fs::read_dir(&directory)?.collect::<Result<Vec<_>, _>>()?
    } else {
        Vec::new()
    };
    files.sort_by_key(|entry| entry.file_name());

    let mut modules = String::new();
    let mut registrations = String::new();
    for file in files {
        let path = file.path();
        if !file.file_type()?.is_file()
            || path.extension().and_then(|ext| ext.to_str()) != Some("rs")
        {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if stem.is_empty()
            || !stem
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        {
            return Err(
                io::Error::other(format!("invalid resource filename: {}", path.display())).into(),
            );
        }
        let module = format!("resource_{stem}");
        modules.push_str(&format!("#[path = {:?}] pub mod {module};\n", path));
        registrations.push_str(&format!("$api.register::<$crate::{module}::Model>()?;\n"));
    }
    modules.push_str(&format!(
        "#[macro_export] macro_rules! register_resources {{ ($api:expr) => {{{{ {registrations} }}}}; }}\n"
    ));
    fs::write(
        PathBuf::from(env::var("OUT_DIR")?).join("resources.rs"),
        modules,
    )?;
    Ok(())
}