#![feature(exit_status_error)]
#![feature(async_closure)]

pub mod builder;
pub mod devenv;
pub mod qemu;

use anyhow::Context;
use anyhow::Result;
use async_process::Stdio;
use std::process::Command;

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
