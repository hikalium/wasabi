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
    project_root: Option<String>,
}

async fn test_os_efi_is_valid_efi_app(dev_env: &DevEnv) -> Result<()> {
    eprintln!("{dev_env:?}");
    let path_to_efi = dev_env.wasabi_efi_path();
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

async fn test_wasabi_os_is_bootable(dev_env: &DevEnv) -> Result<()> {
    let (mut qemu, _rootfs) =
        Qemu::launch_with_wasabi_os(dev_env.wasabi_efi_path(), dev_env.ovmf_path())?;
    sleep(Duration::from_millis(10000));
    qemu.kill().await?;
    Ok(())
}

async fn run_tests(dev_env: &DevEnv) -> Result<()> {
    test_os_efi_is_valid_efi_app(dev_env).await?;
    test_qemu_is_killable_via_monitor().await?;
    test_wasabi_os_is_bootable(dev_env).await?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Args = argh::from_env();
    if let Some(project_root_override) = &args.project_root {
        std::env::set_current_dir(project_root_override)?;
        println!("Using project root : {project_root_override}");
    }
    std::env::remove_var("RUSTUP_TOOLCHAIN");
    let dev_env = detect_dev_env()?;
    let root_dir = dev_env.wasabi_workspace_root_dir();
    println!("Entering dir : {root_dir}");
    std::env::set_current_dir(root_dir)?;
    build_wasabi()?;
    run_tests(&dev_env).await?;
    Ok(())
}
