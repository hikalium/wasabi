#![feature(exit_status_error)]

use anyhow::Context;
use anyhow::Result;
use async_process::Stdio;
use serde::Deserialize;
use serde::Serialize;
use std::process::Command;

/// Subset of `cargo metadata --format-version 1`
#[derive(Serialize, Deserialize, Debug)]
struct CargoMetadataV1 {
    /// Full-path of the target directory
    target_directory: String,
    /// Full-path of the workspace directory
    workspace_root: String,
}

fn run_shell_cmd(cmd: &str) -> Result<(String, String)> {
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

fn run_shell_cmd_at_nocapture(cmd: &str, cwd: &str) -> Result<()> {
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
        .stdin(Stdio::piped())
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

pub fn dump_dev_env() -> Result<()> {
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

    let cargo_metadata = &run_shell_cmd("cargo metadata --format-version 1")?.0;
    let cargo_metadata: CargoMetadataV1 = serde_json::from_str(cargo_metadata.as_str())?;
    println!("Cargo metadata  : {cargo_metadata:?}");
    Ok(())
}

pub fn build_wasabi() -> Result<()> {
    run_shell_cmd_at_nocapture("cargo build --release", "./os/")?;
    Ok(())
}
