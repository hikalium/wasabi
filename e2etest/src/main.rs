#![feature(exit_status_error)]

use anyhow::bail;
use anyhow::Result;

fn main() -> Result<()> {
    bail!("Please run `cargo test` instead.");
}

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
        let mut qemu = Qemu::new(dev_env.ovmf_path())?;
        let _rootfs = qemu.launch_with_wasabi_os(dev_env.wasabi_efi_path())?;
        qemu.wait_until_serial_output_contains("usb_hid_keyboard is ready")?;
        // Confirm that TEST_STRING is not typed into the machine yet.
        let output = qemu.read_serial_output()?;
        assert!(!output.contains(TEST_STRING));
        qemu.send_monitor_cmd("sendkey ret").await?;
        qemu.wait_until_serial_output_contains("Welcome to WasabiOS!")?;
        // OS is now booted. Let's type chars into the machine.
        for c in TEST_STRING.chars() {
            qemu.send_monitor_cmd(&format!("sendkey {c}")).await?;
        }
        qemu.send_monitor_cmd("sendkey ret").await?;
        // Now, verify if the TEST_STRING is in the serial output.
        qemu.wait_until_serial_output_contains(TEST_STRING)?;
        qemu.kill().await?;
        Ok(())
    }

    #[tokio::test]
    async fn app_hello0_is_working() -> Result<()> {
        const APP_NAME: &str = "hello0";
        const EXPECTED_OUTPUT: &str = "**** Hello from an app!";
        let dev_env = DevEnv::new()?;
        let app_bin_path = dev_env.build_builtin_app(APP_NAME)?;
        let mut qemu = Qemu::new(dev_env.ovmf_path())?;
        let _rootfs = qemu
            .launch_with_wasabi_os_and_files(dev_env.wasabi_efi_path(), &[app_bin_path.as_str()])?;
        qemu.wait_until_serial_output_contains("usb_hid_keyboard is ready")?;
        // Confirm that TEST_STRING is not typed into the machine yet.
        let output = qemu.read_serial_output()?;
        assert!(!output.contains(EXPECTED_OUTPUT));
        qemu.send_monitor_cmd("sendkey ret").await?;
        qemu.wait_until_serial_output_contains("Welcome to WasabiOS!")?;
        // OS is now booted. Let's type chars into the machine.
        for c in APP_NAME.chars() {
            qemu.send_monitor_cmd(&format!("sendkey {c}")).await?;
        }
        qemu.send_monitor_cmd("sendkey ret").await?;
        // Now, verify if the TEST_STRING is in the serial output.
        qemu.wait_until_serial_output_contains(EXPECTED_OUTPUT)?;
        qemu.kill().await?;
        Ok(())
    }
    #[tokio::test]
    async fn app_rev_is_working() -> Result<()> {
        const APP_NAME: &str = "rev";
        const INPUT_STRING: &str = "wasabios";
        const APP_READY_OUTPUT: &str = "Type q and hit Enter";
        const EXPECTED_OUTPUT: &str = "soibasaw";
        let dev_env = DevEnv::new()?;
        let app_bin_path = dev_env.build_builtin_app(APP_NAME)?;
        let mut qemu = Qemu::new(dev_env.ovmf_path())?;
        let _rootfs = qemu
            .launch_with_wasabi_os_and_files(dev_env.wasabi_efi_path(), &[app_bin_path.as_str()])?;
        qemu.wait_until_serial_output_contains("usb_hid_keyboard is ready")?;
        // Confirm that TEST_STRING is not typed into the machine yet.
        let output = qemu.read_serial_output()?;
        assert!(!output.contains(EXPECTED_OUTPUT));
        qemu.send_monitor_cmd("sendkey ret").await?;
        qemu.wait_until_serial_output_contains("Welcome to WasabiOS!")?;
        // OS is now booted. Let's type chars into the machine.
        for c in APP_NAME.chars() {
            qemu.send_monitor_cmd(&format!("sendkey {c}")).await?;
        }
        qemu.send_monitor_cmd("sendkey ret").await?;
        // Wait app to be ready
        qemu.wait_until_serial_output_contains(APP_READY_OUTPUT)?;
        // Send chars to the app
        for c in INPUT_STRING.chars() {
            qemu.send_monitor_cmd(&format!("sendkey {c}")).await?;
        }
        qemu.send_monitor_cmd("sendkey ret").await?;
        // Now, verify if the EXPECTED_OUTPUT is in the serial output.
        qemu.wait_until_serial_output_contains(EXPECTED_OUTPUT)?;
        qemu.kill().await?;
        Ok(())
    }
}
