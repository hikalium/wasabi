#![feature(exit_status_error)]

use anyhow::Result;
use argh::FromArgs;
use e2etest::build_wasabi;
use e2etest::detect_dev_env;
use e2etest::qemu::Qemu;
use e2etest::run_shell_cmd;
use e2etest::DevEnv;
use std::thread::sleep;
use std::time::Duration;

#[derive(FromArgs, PartialEq, Debug)]
/// WasabiOS end-to-end test runner
struct Args {
    /// absolute path for the WasabiOS project
    #[argh(option)]
    project_root: String,
}

async fn test_os_efi_is_valid_efi_app(dev_env: &DevEnv) -> Result<()> {
    eprintln!("{dev_env:?}");
    let path_to_efi = dev_env.wasabi_efi_path();
    let path_to_efi = path_to_efi.to_string_lossy();
    let cmd = format!("file '{path_to_efi}'");
    let (stdout, _) = run_shell_cmd(&cmd)?;
    eprintln!("{stdout}");
    assert!(stdout.contains("EFI application"));
    assert!(stdout.contains("x86-64"));
    Ok(())
}

async fn test_qemu_is_killable_via_monitor() -> Result<()> {
    let mut qemu = Qemu::launch_without_os()?;
    sleep(Duration::from_millis(500));
    qemu.kill().await?;
    Ok(())
}

async fn run_tests(dev_env: &DevEnv) -> Result<()> {
    test_os_efi_is_valid_efi_app(dev_env).await?;
    test_qemu_is_killable_via_monitor().await?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Args = argh::from_env();
    let project_root = args.project_root;
    println!("Project root  : {project_root}");

    std::env::set_current_dir(project_root)?;
    std::env::remove_var("RUSTUP_TOOLCHAIN");

    let dev_env = detect_dev_env()?;
    build_wasabi()?;
    run_tests(&dev_env).await?;
    Ok(())
}
