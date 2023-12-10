use crate::run_shell_cmd_at_nocapture;
use crate::spawn_shell_cmd_at_nocapture;
use anyhow::anyhow;
use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use retry::delay::Fixed;
use retry::retry;
use retry::OperationResult;
use std::io;
use std::process;
use std::process::ExitStatus;
use tokio::io::AsyncWriteExt;
use tokio::net::UnixStream;

pub struct RootFs {
    path: String,
}
impl RootFs {
    pub fn new() -> Result<Self> {
        let path = tempfile::tempdir()?.path().to_string_lossy().to_string();

        Ok(Self { path })
    }
    pub fn copy_boot_loader_from(&self, path_to_efi: &str) -> Result<()> {
        let rootfs = &self.path;
        run_shell_cmd_at_nocapture(
            &format!(
                "mkdir -p {rootfs}/EFI/BOOT && cp {path_to_efi} {rootfs}/EFI/BOOT/BOOTX64.EFI"
            ),
            ".",
        )
    }
}

pub struct Qemu {
    proc: Option<process::Child>,
    monitor_sock_path: String,
    path_to_ovmf: String,
}

impl Qemu {
    pub fn new(path_to_ovmf: &str) -> Result<Self> {
        let monitor_sock_path = tempfile::NamedTempFile::new()?;
        let monitor_sock_path = monitor_sock_path
            .into_temp_path()
            .as_os_str()
            .to_string_lossy()
            .to_string();
        Ok(Self {
            proc: None,
            monitor_sock_path,
            path_to_ovmf: path_to_ovmf.to_string(),
        })
    }
    fn gen_base_args(&self) -> String {
        format!(
            "qemu-system-x86_64 \
                -machine q35 \
                -cpu qemu64 \
                -smp 4 \
                --no-reboot \
                -monitor unix:{},server,nowait \
                -bios {}",
            self.monitor_sock_path, self.path_to_ovmf,
        )
    }
    pub fn launch_without_os(&mut self) -> Result<()> {
        if self.proc.is_some() {
            bail!("Already launched: {:?}", self.proc);
        }
        let base_args = self.gen_base_args();
        let proc = spawn_shell_cmd_at_nocapture(&base_args, ".")?;
        eprintln!("QEMU (without OS) spawned: id = {}", proc.id());
        self.proc = Some(proc);
        Ok(())
    }
    pub fn launch_with_wasabi_os(&mut self, path_to_efi: &str) -> Result<RootFs> {
        if self.proc.is_some() {
            bail!("Already launched: {:?}", self.proc);
        }
        let root_fs = RootFs::new()?;
        root_fs.copy_boot_loader_from(path_to_efi)?;
        let base_args = self.gen_base_args();
        let proc = spawn_shell_cmd_at_nocapture(
            &format!("{base_args} -drive format=raw,file=fat:rw:{}", root_fs.path),
            ".",
        )?;
        eprintln!("QEMU (with WasabiOS) spawned: id = {}", proc.id());
        self.proc = Some(proc);
        Ok(root_fs)
    }
    fn wait_to_be_killed(&mut self) -> Result<()> {
        let status: ExitStatus = retry(Fixed::from_millis(500).take(6), || {
            if let Some(proc) = self.proc.as_mut() {
                if let Ok(Some(status)) = proc.try_wait() {
                    OperationResult::Ok(status)
                } else {
                    OperationResult::Retry("waiting")
                }
            } else {
                OperationResult::Err("proc was null")
            }
        })
        .unwrap();
        status
            .exit_ok()
            .context("QEMU should exit succesfully with quit command, but got error")?;
        eprintln!("QEMU exited succesfully");
        Ok(())
    }
    async fn send_monitor_cmd(&mut self, cmd: &str) -> Result<()> {
        eprintln!("send_monitor_cmd: Sending {cmd:?}...");
        let mut stream = UnixStream::connect(&self.monitor_sock_path)
            .await
            .context("Failed to open a UNIX domain socket for QEMU monitor")?;
        let mut bytes_done = 0;

        loop {
            // Wait for the socket to be writable
            stream.writable().await?;

            // Try to write data, this may still fail with `WouldBlock`
            // if the readiness event is a false positive.
            match stream.try_write(cmd[bytes_done..].as_bytes()) {
                Ok(n) => {
                    bytes_done += n;
                    if bytes_done == cmd.len() {
                        break;
                    }
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                    continue;
                }
                Err(e) => {
                    return Err(e.into());
                }
            }
        }
        stream.flush().await?;
        eprintln!("Sent QEMU monitor command: {cmd:?}");
        loop {
            // Wait for the socket to be readable
            stream.readable().await?;

            // Creating the buffer **after** the `await` prevents it from
            // being stored in the async task.
            let mut buf = [0; 4096];

            // Try to read data, this may still fail with `WouldBlock`
            // if the readiness event is a false positive.
            match stream.try_read(&mut buf) {
                Ok(0) => break,
                Ok(_) => {
                    eprint!("recv: {}", &String::from_utf8_lossy(&buf));
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                    continue;
                }
                Err(e) => {
                    return Err(e.into());
                }
            }
        }
        Ok(())
    }
    pub async fn kill(&mut self) -> Result<()> {
        self.send_monitor_cmd("quit\n").await?;
        self.wait_to_be_killed()
    }
}

impl Drop for Qemu {
    fn drop(&mut self) {
        if let Some(mut proc) = self.proc.take() {
            if proc.try_wait().is_err() {
                // looks like the process is still running so kill it
                proc.kill()
                    .context(anyhow!("Failed to kill QEMU (id = {})", proc.id()))
                    .unwrap();
            }
            eprintln!(
                "QEMU (pid = {}) is killed since it is dropped. status: {:?}",
                proc.id(),
                proc.try_wait()
            );
        }
    }
}
