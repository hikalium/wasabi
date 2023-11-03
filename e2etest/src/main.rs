use anyhow::Context;
use anyhow::Result;
use argh::FromArgs;
use std::process::Command;

#[derive(FromArgs, PartialEq, Debug)]
/// WasabiOS end-to-end test runner
struct Args {
    /// absolute path for the WasabiOS project
    #[argh(option)]
    project_root: String,
}

fn run_shell_cmd(cmd: &str) -> Result<(String, String)> {
    let result = Command::new("bash")
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

fn get_first_line(s: &str) -> String {
    let s = s.split('\n').collect::<Vec<&str>>();
    // the first line of text should always be there, so this unwrap will not panic
    s.first().unwrap().to_string()
}

fn main() -> Result<()> {
    let args: Args = argh::from_env();
    let project_root = args.project_root;
    println!("Project root  : {project_root}");

    std::env::set_current_dir(project_root)?;
    std::env::remove_var("RUSTUP_TOOLCHAIN");

    let (qemu_path, _) = run_shell_cmd("which qemu-system-x86_64")?;
    println!("Using QEMU at : {qemu_path}");
    let qemu_version = get_first_line(&run_shell_cmd(&format!("{qemu_path} --version"))?.0);
    println!("QEMU version  : {qemu_version}");

    let (make_path, _) = run_shell_cmd("which make")?;
    println!("Using make at : {make_path}");
    let make_version = get_first_line(&run_shell_cmd(&format!("{make_path} --version"))?.0);
    println!("make version  : {make_version}");

    let rust_toolchain = get_first_line(&run_shell_cmd("rustup show active-toolchain")?.0);
    println!("Rust version  : {rust_toolchain}");
    Ok(())
}
