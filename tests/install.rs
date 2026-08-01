#![cfg(unix)]

use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
};

use uuid::Uuid;

fn root() -> PathBuf {
    std::env::temp_dir().join(format!("longrun-install-{}", Uuid::now_v7()))
}

fn release(root: &Path, target: &str, checksum: &str) {
    let staging = root.join("staging");
    fs::create_dir_all(&staging).expect("staging");
    let binary = staging.join("longrun");
    fs::write(&binary, "#!/bin/sh\nprintf longrun\n").expect("binary");
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).expect("mode");
    let archive = root.join(format!("longrun-{target}.tar.gz"));
    assert!(
        Command::new("tar")
            .args(["-czf", archive.to_str().expect("archive"), "-C"])
            .arg(&staging)
            .arg("longrun")
            .status()
            .expect("tar")
            .success()
    );
    fs::write(
        root.join(format!("longrun-{target}.tar.gz.sha256")),
        format!("{checksum}  longrun-{target}.tar.gz\n"),
    )
    .expect("checksum");
}

fn sha256(path: &Path) -> String {
    let output = Command::new("shasum")
        .args(["-a", "256"])
        .arg(path)
        .output()
        .expect("shasum");
    assert!(output.status.success());
    String::from_utf8(output.stdout)
        .expect("hash")
        .split_whitespace()
        .next()
        .expect("digest")
        .into()
}

fn install(root: &Path) -> std::process::Output {
    let install_dir = root.join("bin");
    Command::new("sh")
        .arg("install.sh")
        .env("LONGRUN_BASE_URL", format!("file://{}", root.display()))
        .env("LONGRUN_INSTALL_DIR", &install_dir)
        .env("LONGRUN_OS", "Linux")
        .env("LONGRUN_ARCH", "x86_64")
        .env("LONGRUN_VERSION", "vtest")
        .output()
        .expect("run installer")
}

#[test]
fn installer_selects_linux_archive_verifies_checksum_and_installs_the_binary() {
    let root = root();
    fs::create_dir_all(&root).expect("root");
    let target = "x86_64-unknown-linux-gnu";
    let archive = root.join(format!("longrun-{target}.tar.gz"));
    release(&root, target, "");
    let checksum = sha256(&archive);
    fs::write(
        root.join(format!("longrun-{target}.tar.gz.sha256")),
        format!("{checksum}  longrun-{target}.tar.gz\n"),
    )
    .expect("checksum");

    let output = install(&root);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let installed = root.join("bin/longrun");
    assert!(installed.is_file());
    assert_eq!(
        Command::new(&installed)
            .output()
            .expect("installed longrun")
            .stdout,
        b"longrun"
    );
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn installer_refuses_a_bad_checksum_without_installing_a_binary() {
    let root = root();
    fs::create_dir_all(&root).expect("root");
    let target = "x86_64-unknown-linux-gnu";
    release(
        &root,
        target,
        "0000000000000000000000000000000000000000000000000000000000000000",
    );

    let output = install(&root);
    assert!(!output.status.success());
    assert!(!root.join("bin/longrun").exists());
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn installer_refuses_an_archive_without_the_longrun_binary() {
    let root = root();
    fs::create_dir_all(root.join("staging")).expect("staging");
    let target = "x86_64-unknown-linux-gnu";
    fs::write(root.join("staging/not-longrun"), "not a binary").expect("fixture");
    let archive = root.join(format!("longrun-{target}.tar.gz"));
    assert!(
        Command::new("tar")
            .args(["-czf", archive.to_str().expect("archive"), "-C"])
            .arg(root.join("staging"))
            .arg("not-longrun")
            .status()
            .expect("tar")
            .success()
    );
    fs::write(
        root.join(format!("longrun-{target}.tar.gz.sha256")),
        format!("{}  longrun-{target}.tar.gz\n", sha256(&archive)),
    )
    .expect("checksum");

    let output = install(&root);
    assert!(!output.status.success());
    assert!(!root.join("bin/longrun").exists());
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn package_metadata_and_release_workflow_publish_verifiable_supported_artifacts() {
    let manifest: toml::Value =
        toml::from_str(&fs::read_to_string("Cargo.toml").expect("Cargo manifest")).expect("TOML");
    let package = manifest["package"].as_table().expect("package metadata");
    for field in [
        "homepage",
        "documentation",
        "readme",
        "keywords",
        "categories",
    ] {
        assert!(package.contains_key(field), "missing package.{field}");
    }

    let workflow = fs::read_to_string(".github/workflows/release.yml").expect("release workflow");
    for target in [
        "x86_64-apple-darwin",
        "aarch64-apple-darwin",
        "x86_64-unknown-linux-gnu",
        "x86_64-pc-windows-msvc",
    ] {
        assert!(workflow.contains(target), "missing release target {target}");
    }
    for required in [
        "permissions:",
        "contents: write",
        "sha256sum",
        "shasum -a 256",
        "gh release create",
        "longrun doctor",
    ] {
        assert!(
            workflow.contains(required),
            "missing release workflow {required}"
        );
    }
}
