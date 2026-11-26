#![cfg(unix)]

use std::{fs, os::unix::fs::PermissionsExt, path::Path, process::Command};

fn rapid(project: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_lilyco-rapid"));
    command
        .current_dir(project)
        .env_remove("RAPID_CONTAINER_RUNTIME")
        .env_remove("RAPID_DATABASE_URL")
        .env_remove("RAPID_CONFIG")
        .env("PATH", project.join("bin"));
    command
}

fn script(project: &Path, name: &str, source: &str) {
    let path = project.join("bin").join(name);
    fs::create_dir_all(project.join("bin")).unwrap();
    fs::write(&path, format!("#!/bin/sh\n{source}\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

#[test]
fn data_uses_selected_compose_runtime_and_passes_local_connection_to_cargo() {
    let folder = tempfile::tempdir().unwrap();
    let project = folder.path();
    fs::write(
        project.join(".env"),
        "RAPID_LOCAL_DB=postgres\nRAPID_DB_PASSWORD=abcdef\n",
    )
    .unwrap();
    let compose = r#"
printf '%s %s\n' "${0##*/}" "$*" >> compose.log
case "$*" in
  'compose up -d --wait database') exit 0 ;;
  'compose port database 5432') printf '127.0.0.1:49444\n' ;;
  *) exit 70 ;;
esac
"#;
    script(project, "docker", compose);
    script(project, "kiri", compose);
    script(
        project,
        "cargo",
        r#"
test "$*" = 'run --locked -p rapid-server -- export sample.json' &&
test "$RAPID_DATABASE_URL" = 'postgres://rapid:abcdef@127.0.0.1:49444/rapid'
"#,
    );
    for args in [
        vec!["data", "export", "sample.json"],
        vec!["data", "export", "sample.json", "--runtime", "docker"],
    ] {
        let output = rapid(project)
            .env("RAPID_CONTAINER_RUNTIME", "kiri")
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    assert_eq!(
        fs::read_to_string(project.join("compose.log")).unwrap(),
        concat!(
            "kiri compose up -d --wait database\n",
            "kiri compose port database 5432\n",
            "docker compose up -d --wait database\n",
            "docker compose port database 5432\n",
        )
    );
}

#[test]
fn api_only_dev_reaches_the_rust_build_without_frontend_tools() {
    let folder = tempfile::tempdir().unwrap();
    let project = folder.path();
    fs::write(project.join("Rapid.toml"), "").unwrap();
    for name in ["server", "core", "web"] {
        fs::create_dir(project.join(name)).unwrap();
    }
    fs::write(project.join("web/package.json"), "{}").unwrap();
    script(project, "cargo", "exit 72");

    let output = rapid(project)
        .args(["dev", "--api-only", "--runtime", "kiri"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("cargo exited with exit status: 72"));
}
