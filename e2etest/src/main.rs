#![feature(exit_status_error)]

use anyhow::bail;
use anyhow::Result;
use e2etest::detect_dev_env;
use e2etest::qemu::Qemu;
use e2etest::run_shell_cmd;
use e2etest::DevEnv;
use std::thread::sleep;
use std::time::Duration;

fn test_setup() -> Result<DevEnv> {
    std::env::remove_var("RUSTUP_TOOLCHAIN");
    let dev_env = detect_dev_env()?;
    let root_dir = dev_env.wasabi_workspace_root_dir();
    println!("Entering dir : {root_dir}");
    std::env::set_current_dir(root_dir)?;
    Ok(dev_env)
}

#[tokio::test]
async fn test_os_efi_is_valid_efi_app() -> Result<()> {
    let dev_env = test_setup()?;
    let path_to_efi = dev_env.wasabi_efi_path();
    let cmd = format!("file '{path_to_efi}'");
    let (stdout, _) = run_shell_cmd(&cmd)?;
    eprintln!("{stdout}");
    assert!(stdout.contains("EFI application"));
    assert!(stdout.contains("x86-64"));
    Ok(())
}

#[tokio::test]
async fn qemu_is_killable_via_monitor() -> Result<()> {
    let _dev_env = test_setup()?;
    let mut qemu = Qemu::new()?;
    qemu.launch_without_os()?;
    sleep(Duration::from_millis(500));
    qemu.kill().await?;
    Ok(())
}

#[tokio::test]
async fn wasabi_os_is_bootable() -> Result<()> {
    let dev_env = test_setup()?;
    let mut qemu = Qemu::new()?;
    let _rootfs = qemu.launch_with_wasabi_os(dev_env.wasabi_efi_path(), dev_env.ovmf_path())?;
    sleep(Duration::from_millis(3000));
    qemu.kill().await?;
    Ok(())
}

fn main() -> Result<()> {
    bail!("Please run `cargo test` instead.");
}
