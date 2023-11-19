#![feature(exit_status_error)]

use anyhow::Context;
use anyhow::Result;
use argh::FromArgs;
use async_process::Stdio;
use futures::executor::block_on;
use futures::io::BufReader;
use futures::io::Lines;
use serde::Deserialize;
use serde::Serialize;
use std::io::Write;
use std::process::Command;
use std::thread::sleep;
use std::time::Duration;

/// Subset of `cargo metadata --format-version 1`
#[derive(Serialize, Deserialize, Debug)]
struct CargoMetadataV1 {
    /// Full-path of the target directory
    target_directory: String,
    /// Full-path of the workspace directory
    workspace_root: String,
}

pub type AsyncLinesReader<T> = Lines<BufReader<T>>;

#[derive(FromArgs, PartialEq, Debug)]
/// WasabiOS end-to-end test runner
struct Args {
    /// absolute path for the WasabiOS project
    #[argh(option)]
    project_root: String,
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

fn spawn_shell_cmd_at_nocapture(cmd: &str, cwd: &str) -> Result<std::process::Child> {
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

fn dump_dev_env() -> Result<()> {
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

fn build_wasabi() -> Result<()> {
    run_shell_cmd_at_nocapture("cargo build --release", "./os/")?;
    Ok(())
}

fn test_qemu_is_killable_via_monitor() -> Result<()> {
    let mut child = spawn_shell_cmd_at_nocapture("qemu-system-x86_64 -monitor stdio", ".")?;
    eprintln!("QEMU spawned");
    sleep(Duration::from_millis(1000));
    let mut stdin = child.stdin.take().context("stdin was None")?;
    stdin.write_all(b"quit\n")?;
    let status = child.wait()?;
    status
        .exit_ok()
        .context("QEMU should exit succesfully with quit command, but got error")?;
    eprintln!("QEMU exited succesfully");
    Ok(())
}
async fn run_tests() -> Result<()> {
    test_qemu_is_killable_via_monitor()?;
    Ok(())
}

fn main() -> Result<()> {
    let args: Args = argh::from_env();
    let project_root = args.project_root;
    println!("Project root  : {project_root}");

    std::env::set_current_dir(project_root)?;
    std::env::remove_var("RUSTUP_TOOLCHAIN");

    dump_dev_env()?;
    build_wasabi()?;
    block_on(async {
        if let Err(e) = run_tests().await {
            eprintln!("{e}");
        }
    });

    Ok(())
}
