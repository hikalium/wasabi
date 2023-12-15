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
        eprintln!("build done");
        let mut qemu = Qemu::new(dev_env.ovmf_path())?;
        let _rootfs = qemu.launch_with_wasabi_os(dev_env.wasabi_efi_path())?;
        sleep(Duration::from_millis(3000));
        qemu.kill().await?;
        let output = qemu.read_com2_output()?;
        eprintln!("{output}");
        bail!("tmp");
        Ok(())
    }
}

fn main() -> Result<()> {
    bail!("Please run `cargo test` instead.");
}
