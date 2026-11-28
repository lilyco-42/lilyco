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
fn database_drivers_and_gateways_generate_independent_configuration() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let output = rapid(
        root,
        &[
            "new",
            "dashboard",
            "--db",
            "postgres",
            "--db-driver",
            "seaorm",
            "--gateway",
            "pingap",
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let workspace = root.join("dashboard");
    let settings = fs::read_to_string(workspace.join("Rapid.toml")).unwrap();
    assert!(settings.contains("engine = \"postgres\""));
    assert!(settings.contains("driver = \"seaorm\""));
    let manifest = fs::read_to_string(workspace.join("server/Cargo.toml")).unwrap();
    assert!(manifest.contains("default = [\"seaorm\"]"));
    let dockerfile = fs::read_to_string(workspace.join("ops/Dockerfile.web")).unwrap();
    assert!(dockerfile.contains("FROM vicanso/pingap:"));
    assert!(workspace.join("ops/Pingap.toml").is_file());
    assert!(workspace.join("compose.override.yaml").is_file());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            workspace
                .join(".env")
                .metadata()
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    assert!(
        !rapid(
            root,
            &["new", "invalid", "--db", "mysql", "--db-driver", "rusqlite"]
        )
        .status
        .success()
    );
    assert!(!root.join("invalid").exists());
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

#[test]
fn frontend_preview_leaves_existing_project_files_in_place() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    assert!(
        rapid(
            root,
            &["new", "switcher", "--ui", "react", "--ui-kit", "native"]
        )
        .status
        .success()
    );
    let project = root.join("switcher");
    let entry = project.join("web/src/components/button.tsx");
    let original = fs::read(&entry).unwrap();
    assert!(
        std::str::from_utf8(&original)
            .unwrap()
            .contains("rapid-button")
    );
    let config = fs::read(project.join("Rapid.toml")).unwrap();

    let invalid = ["generate", "frontend", "--ui", "vue", "--ui-kit", "shadcn"];
    assert!(!rapid(&project, &invalid).status.success());
    assert!(!project.join("web-next").exists());
    let preview = ["generate", "frontend", "--ui", "svelte"];
    assert!(rapid(&project, &preview).status.success());
    assert!(!rapid(&project, &preview).status.success());
    assert!(project.join("web-next/src/App.svelte").is_file());
    assert!(project.join("web-next/src/lib/api.ts").is_file());
    assert_eq!(fs::read(&entry).unwrap(), original);
    assert_eq!(fs::read(project.join("Rapid.toml")).unwrap(), config);
}
