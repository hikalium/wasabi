use crate::run_shell_cmd_at_nocapture;
use crate::spawn_shell_cmd_at_nocapture;
use anyhow::anyhow;
use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use retry::delay::Fixed;
use retry::retry;
use retry::OperationResult;
use std::fs;
use std::io;
use std::process;
use std::process::ExitStatus;
use std::time::Duration;
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
    work_dir: tempfile::TempDir,
    path_to_ovmf: String,
}

impl Qemu {
    const MONITOR_SOCKET_NAME: &str = "monitor.sock";
    pub fn new(path_to_ovmf: &str) -> Result<Self> {
        Ok(Self {
            proc: None,
            path_to_ovmf: path_to_ovmf.to_string(),
            work_dir: tempfile::TempDir::new()?,
        })
    }
    fn work_dir_path(&self) -> Result<&str> {
        self.work_dir
            .path()
            .to_str()
            .context("work_dir path for Qemu is not a valid UTF-8 string")
    }
    fn monitor_sock_path(&self) -> Result<String> {
        let work_dir = self.work_dir_path()?;
        let monitor_socket_name = Self::MONITOR_SOCKET_NAME;
        Ok(format!("{work_dir}/{monitor_socket_name}"))
    }
    fn gen_base_args(&self) -> Result<String> {
        let work_dir = self.work_dir_path()?;
        let path_to_ovmf = self.path_to_ovmf.as_str();
        let monitor_sock_path = self.monitor_sock_path()?;
        Ok(format!(
            "qemu-system-x86_64 \
                -machine q35 \
                -cpu qemu64 \
                -smp 4 \
                -device qemu-xhci \
                --no-reboot \
                -monitor unix:{monitor_sock_path},server,nowait \
                -device isa-debug-exit,iobase=0xf4,iosize=0x01 \
                -netdev user,id=net1 \
                -device rtl8139,netdev=net1 \
                -object filter-dump,id=f2,netdev=net1,file={work_dir}/net1.pcap \
                -m 1024M \
                -drive format=raw,file=fat:rw:mnt \
                -chardev file,id=char_com1,mux=on,path={work_dir}/com1.txt \
                -chardev file,id=char_com2,mux=on,path={work_dir}/com2.txt \
                -serial chardev:char_com1 \
                -serial chardev:char_com2 \
                -display none \
                -bios {path_to_ovmf}",
        ))
    }
    pub fn read_com2_output(&mut self) -> Result<String> {
        let work_dir = self.work_dir_path()?;
        fs::read_to_string(format!("{work_dir}/com2.txt")).context("Failed to read com2 output")
    }
    pub fn launch_without_os(&mut self) -> Result<()> {
        if self.proc.is_some() {
            bail!("Already launched: {:?}", self.proc);
        }
        let base_args = self.gen_base_args()?;
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
        let base_args = self.gen_base_args()?;
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
        let mut retry_count = 0;
        let mut stream = loop {
            let sock = UnixStream::connect(&self.monitor_sock_path()?)
                .await
                .context("Failed to open a UNIX domain socket for QEMU monitor");
            if let Ok(sock) = sock {
                break sock;
            } else if retry_count < 10 {
                eprintln!("{sock:?}");
                std::thread::sleep(Duration::from_millis(500));
                retry_count += 1;
                continue;
            } else {
                bail!("{sock:?}")
            }
        };
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
