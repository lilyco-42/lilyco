use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

fn rapid(path: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_lilyco-rapid"))
        .current_dir(path)
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn presets_generate_portable_paths_and_refuse_to_overwrite() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    for (name, kind, fullstack) in [("notes", "fullstack", true), ("service", "api", false)] {
        let output = rapid(root, &["new", name, "--preset", kind]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let product = root.join(name);
        assert!(product.join("ops/lilyco-no_std/src/lib.rs").is_file());
        assert!(product.join("compose.yaml").is_file());
        assert_eq!(product.join("web/package.json").exists(), fullstack);
        assert!(
            fs::read_to_string(product.join("Cargo.toml"))
                .unwrap()
                .contains("ops/lilyco-no_std")
        );
        assert_eq!(product.join("web/pnpm-workspace.yaml").exists(), fullstack);
        let metadata = Command::new("cargo")
            .current_dir(&product)
            .args(["metadata", "--locked", "--no-deps", "--format-version", "1"])
            .output()
            .unwrap();
        assert!(
            metadata.status.success(),
            "{}",
            String::from_utf8_lossy(&metadata.stderr)
        );
        assert!(!rapid(root, &["new", name]).status.success());
    }
    assert!(!rapid(root, &["new", "../escape"]).status.success());
}

#[test]
fn resource_generator_checks_input_and_writes_once() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    assert!(rapid(root, &["new", "journal"]).status.success());
    let project = root.join("journal");
    let generate = [
        "generate",
        "resource",
        "log-entries",
        "--field",
        "type:string",
        "--field",
        "count:i32",
    ];
    assert!(rapid(&project, &generate).status.success());
    assert!(!rapid(&project, &generate).status.success());
    let source = fs::read_to_string(project.join("core/resources/log_entries.rs")).unwrap();
    assert!(source.contains("pub r#type: String"));
    assert!(source.contains("pub r#count: i32"));
    assert!(
        !rapid(
            &project,
            &[
                "generate", "resource", "oops", "--field", "x:string", "--field", "x:bool"
            ]
        )
        .status
        .success()
    );
    assert!(!project.join("core/resources/oops.rs").exists());
}
