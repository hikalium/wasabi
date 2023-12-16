#![feature(exit_status_error)]

use anyhow::bail;
use anyhow::Result;

#[cfg(test)]
mod test {
    use super::*;
    use e2etest::devenv::DevEnv;
    use e2etest::qemu::Qemu;
    use e2etest::run_shell_cmd;
    use std::thread::sleep;
    use std::time::Duration;
    #[tokio::test]
    async fn os_efi_is_valid_efi_app() -> Result<()> {
        let dev_env = DevEnv::new()?;
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
        let dev_env = DevEnv::new()?;
        let mut qemu = Qemu::new(dev_env.ovmf_path())?;
        qemu.launch_without_os()?;
        sleep(Duration::from_millis(500));
        qemu.kill().await?;
        Ok(())
    }

    #[tokio::test]
    async fn wasabi_os_is_bootable() -> Result<()> {
        let dev_env = DevEnv::new()?;
        e2etest::devenv::build_wasabi()?;
        let mut qemu = Qemu::new(dev_env.ovmf_path())?;
        let _rootfs = qemu.launch_with_wasabi_os(dev_env.wasabi_efi_path())?;
        qemu.wait_until_serial_output_contains("Welcome to WasabiOS!")?;
        qemu.kill().await?;
        Ok(())
    }

    #[tokio::test]
    async fn keyboard_is_working() -> Result<()> {
        const TEST_STRING: &str = "qwerty";
        let dev_env = DevEnv::new()?;
        e2etest::devenv::build_wasabi()?;
        let mut qemu = Qemu::new(dev_env.ovmf_path())?;
        let _rootfs = qemu.launch_with_wasabi_os(dev_env.wasabi_efi_path())?;
        qemu.wait_until_serial_output_contains("INFO: usb_hid_keyboard is ready")?;
        // Confirm that TEST_STRING is not typed into the machine yet.
        let output = qemu.read_serial_output()?;
        assert!(!output.contains(TEST_STRING));
        qemu.send_monitor_cmd("sendkey ret").await?;
        qemu.wait_until_serial_output_contains("Welcome to WasabiOS!")?;
        qemu.send_monitor_cmd("sendkey q").await?;
        qemu.send_monitor_cmd("sendkey w").await?;
        qemu.send_monitor_cmd("sendkey e").await?;
        qemu.send_monitor_cmd("sendkey r").await?;
        qemu.send_monitor_cmd("sendkey t").await?;
        qemu.send_monitor_cmd("sendkey y").await?;
        qemu.send_monitor_cmd("sendkey ret").await?;
        qemu.wait_until_serial_output_contains(TEST_STRING)?;
        qemu.kill().await?;
        Ok(())
    }
}

fn main() -> Result<()> {
    bail!("Please run `cargo test` instead.");
}
