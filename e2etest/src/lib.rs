#![feature(exit_status_error)]
#![feature(async_closure)]

pub mod qemu;

use anyhow::Context;
use anyhow::Result;
use async_process::Stdio;
use serde::Deserialize;
use serde::Serialize;
use std::path::PathBuf;
use std::process::Command;

/// Subset of `cargo metadata --format-version 1`
#[derive(Serialize, Deserialize, Debug)]
pub struct CargoMetadataV1 {
    /// Full-path of the target directory
    target_directory: String,
    /// Full-path of the workspace directory
    workspace_root: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CargoLocateOutput {
    /// Full-path of the workspace root's Cargo.toml
    root: String,
}

pub fn run_shell_cmd(cmd: &str) -> Result<(String, String)> {
    run_shell_cmd_at(cmd, ".")
}

fn run_shell_cmd_at(cmd: &str, cwd: &str) -> Result<(String, String)> {
    let result = Command::new("bash")
        .current_dir(cwd)
        .arg("-c")
        .arg(cmd)
        .output()
        .context("failed to execute shell cmd: {cmd}")?;
    let stdout = String::from_utf8_lossy(&result.stdout);
    let stdout = stdout.trim().to_string();
    let stderr = String::from_utf8_lossy(&result.stderr);
    let stderr = stderr.trim().to_string();
    Ok((stdout, stderr))
}

pub fn run_shell_cmd_at_nocapture(cmd: &str, cwd: &str) -> Result<()> {
    eprintln!("Running: {cmd}");
    let result = Command::new("bash")
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .current_dir(cwd)
        .arg("-c")
        .arg(cmd)
        .output()
        .context("failed to execute shell cmd: {cmd}")?;
    result.status.exit_ok().context("Shell cmd failed")
}

pub fn spawn_shell_cmd_at_nocapture(cmd: &str, cwd: &str) -> Result<std::process::Child> {
    eprintln!("Running: {cmd}");
    Command::new("bash")
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .current_dir(cwd)
        .arg("-c")
        .arg(cmd)
        .spawn()
        .context("failed to execute shell cmd: {cmd}")
}

fn get_first_line(s: &str) -> String {
    let s = s.split('\n').collect::<Vec<&str>>();
    // the first line of text should always be there, so this unwrap will not panic
    s.first().unwrap().to_string()
}

#[derive(Debug)]
pub struct DevEnv {
    wasabi_workspace_root_dir: String,
    wasabi_efi_path: String,
    ovmf_path: String,
}
impl DevEnv {
    pub fn new(cargo_metadata: &CargoMetadataV1) -> Self {
        let wasabi_workspace_root_dir = cargo_metadata.workspace_root.to_string();
        let cargo_target_dir = cargo_metadata.target_directory.to_string();

        let mut efi_path = PathBuf::from(&cargo_target_dir);
        efi_path.push("x86_64-unknown-uefi");
        efi_path.push("release");
        efi_path.push("os.efi");
        let wasabi_efi_path = efi_path.to_string_lossy().to_string();
        Self {
            wasabi_workspace_root_dir: wasabi_workspace_root_dir.clone(),
            wasabi_efi_path,
            ovmf_path: format!("{wasabi_workspace_root_dir}/third_party/ovmf/RELEASEX64_OVMF.fd"),
        }
    }
    pub fn wasabi_efi_path(&self) -> &str {
        &self.wasabi_efi_path
    }
    pub fn wasabi_workspace_root_dir(&self) -> &str {
        &self.wasabi_workspace_root_dir
    }
    pub fn ovmf_path(&self) -> &str {
        &self.ovmf_path
    }
}

pub fn detect_dev_env() -> Result<DevEnv> {
    let (qemu_path, _) = run_shell_cmd("which qemu-system-x86_64")?;
    println!("QEMU path : {qemu_path}");
    let qemu_version = get_first_line(&run_shell_cmd(&format!("{qemu_path} --version"))?.0);
    println!("QEMU version  : {qemu_version}");

    let (make_path, _) = run_shell_cmd("which make")?;
    println!("Make path : {make_path}");
    let make_version = get_first_line(&run_shell_cmd(&format!("{make_path} --version"))?.0);
    println!("Make version  : {make_version}");

    let rust_toolchain = get_first_line(&run_shell_cmd("rustup show active-toolchain")?.0);
    println!("Rust version  : {rust_toolchain}");

    let cargo_workspace_root = &run_shell_cmd("cargo locate-project --workspace")?.0;
    let cargo_workspace_root: CargoLocateOutput =
        serde_json::from_str(cargo_workspace_root.as_str())?;
    let cargo_workspace_root = cargo_workspace_root.root;
    eprintln!("{cargo_workspace_root}");

    let cargo_metadata = &run_shell_cmd(&format!(
        "cargo metadata --format-version 1 --manifest-path {cargo_workspace_root}"
    ))?
    .0;
    let cargo_metadata: CargoMetadataV1 = serde_json::from_str(cargo_metadata.as_str())?;
    println!("Cargo metadata  : {cargo_metadata:?}");
    Ok(DevEnv::new(&cargo_metadata))
}

pub fn build_wasabi() -> Result<()> {
    run_shell_cmd_at_nocapture("cargo build --release", "./os/")?;
    Ok(())
}

/// A setup function to be called at the beginning of every test cases.
pub fn setup() -> Result<DevEnv> {
    std::env::remove_var("RUSTUP_TOOLCHAIN");
    let dev_env = detect_dev_env()?;
    let root_dir = dev_env.wasabi_workspace_root_dir();
    println!("Entering dir : {root_dir}");
    std::env::set_current_dir(root_dir)?;
    Ok(dev_env)
}
