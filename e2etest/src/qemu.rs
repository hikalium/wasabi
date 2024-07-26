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
use std::thread::sleep;
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
    pub fn copy_file_from(&self, src_path: &str) -> Result<()> {
        let rootfs = &self.path;
        run_shell_cmd_at_nocapture(
            &format!("mkdir -p {rootfs} && cp -r {src_path} {rootfs}/"),
            ".",
        )
    }
}

pub struct QemuMonitor {
    stream: UnixStream,
}
impl QemuMonitor {
    pub async fn new(monitor_sock_path: &str) -> Result<Self> {
        let mut retry_count = 0;
        let stream = loop {
            let sock = UnixStream::connect(monitor_sock_path)
                .await
                .context("Failed to open a UNIX domain socket for QEMU monitor");
            if let Ok(sock) = sock {
                break sock;
            } else if retry_count < 100 {
                eprintln!("{sock:?}");
                std::thread::sleep(Duration::from_millis(500));
                retry_count += 1;
                continue;
            } else {
                bail!("{sock:?}")
            }
        };
        // wait for the first prompt on connection
        let mut monitor = Self { stream };
        monitor.wait_until_prompt().await?;
        Ok(monitor)
    }
    pub async fn read_as_much(&mut self) -> Result<String> {
        let mut output = String::new();
        loop {
            // Wait for the socket to be readable
            self.stream.readable().await?;
            // Creating the buffer **after** the `await` prevents it from
            // being stored in the async task.
            let mut buf = [0; 4096];
            // Try to read data, this may still fail with `WouldBlock`
            match self.stream.try_read(&mut buf) {
                Ok(0) => break,
                Ok(_) => {
                    let s = String::from_utf8_lossy(&buf);
                    eprint!("{s}");
                    output.push_str(&s)
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                    // The readiness event was a false positive.
                    break;
                }
                Err(e) => {
                    return Err(e.into());
                }
            }
        }
        Ok(output)
    }
    pub async fn wait_until_prompt(&mut self) -> Result<String> {
        let mut output = String::new();
        loop {
            self.stream.readable().await?;
            let s = self.read_as_much().await?;
            output.push_str(&s);
            if output.contains("(qemu)") {
                break;
            }
        }
        Ok(output)
    }
    pub async fn send(&mut self, cmd: &str) -> Result<()> {
        eprintln!("QemuMonitor::send : Sending {cmd:?}...");
        let mut cmd = cmd.to_string();
        cmd.push('\n');
        let cmd = cmd.as_str();
        let mut bytes_done = 0;
        loop {
            // Wait for the socket to be writable
            self.stream.writable().await?;

            // Try to write data, this may still fail with `WouldBlock`
            // if the readiness event is a false positive.
            match self.stream.try_write(cmd[bytes_done..].as_bytes()) {
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
        self.stream.flush().await?;
        self.wait_until_prompt().await?;
        eprintln!("Sent QEMU monitor command: {cmd:?}");
        Ok(())
    }
}

pub struct Qemu {
    proc: Option<process::Child>,
    work_dir: tempfile::TempDir,
    path_to_ovmf: String,
}

impl Qemu {
    const MONITOR_SOCKET_NAME: &'static str = "monitor.sock";
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
                -netdev user,id=net1,hostfwd=tcp::18080-:18080 \
                -device rtl8139,netdev=net1 \
                -object filter-dump,id=f2,netdev=net1,file={work_dir}/net1.pcap \
                -m 1024M \
                -chardev file,id=char_com1,mux=on,path={work_dir}/com1.txt \
                -chardev file,id=char_com2,mux=on,path={work_dir}/com2.txt \
                -serial chardev:char_com1 \
                -serial chardev:char_com2 \
                -display none \
                -device usb-kbd \
                -bios {path_to_ovmf}",
        ))
    }
    pub fn read_serial_output(&mut self) -> Result<String> {
        let work_dir = self.work_dir_path()?;
        fs::read_to_string(format!("{work_dir}/com2.txt")).context("Failed to read com2 output")
    }
    pub fn wait_until_serial_output_contains(&mut self, s: &str) -> Result<()> {
        const INTERVAL_MS: u64 = 500;
        const TIMEOUT_MS: u64 = 60 * 1000;
        eprintln!("\n>>>>> Waiting serial output `{s}`...");
        let mut duration = 0;
        let mut output = self.read_serial_output().unwrap_or_else(|_| {
            eprint!("serial output was empty: using empty string for now");
            String::new()
        });
        while duration < TIMEOUT_MS {
            let next_output = self.read_serial_output();
            if let Ok(next_output) = &next_output {
                let diff = &next_output[output.len()..];
                eprint!(
                    "{}",
                    String::from_utf8_lossy(&strip_ansi_escapes::strip(diff.replace('\r', "")))
                );
                output += diff;
                if output.contains(s) {
                    eprintln!("OK");
                    return Ok(());
                }
            }
            sleep(Duration::from_millis(INTERVAL_MS));
            eprint!(".");
            duration += INTERVAL_MS;
        }
        bail!("Expected a string `{s}` in the serial output within {TIMEOUT_MS} ms but not found. Output:\n{output}");
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
    pub fn launch_with_wasabi_os_and_files(
        &mut self,
        path_to_efi: &str,
        files: &[&str],
    ) -> Result<RootFs> {
        eprintln!("launch_with_wasabi_os: path_to_efi = {path_to_efi:?}, files = {files:?}",);
        if self.proc.is_some() {
            bail!("Already launched: {:?}", self.proc);
        }
        let root_fs = RootFs::new()?;
        root_fs.copy_boot_loader_from(path_to_efi)?;
        for f in files {
            root_fs.copy_file_from(f)?;
        }
        let base_args = self.gen_base_args()?;
        let proc = spawn_shell_cmd_at_nocapture(
            &format!("{base_args} -drive format=raw,file=fat:rw:{}", root_fs.path),
            ".",
        )?;
        eprintln!("QEMU (with WasabiOS) spawned: id = {}", proc.id());
        self.proc = Some(proc);
        Ok(root_fs)
    }
    pub fn launch_with_wasabi_os(&mut self, path_to_efi: &str) -> Result<RootFs> {
        self.launch_with_wasabi_os_and_files(path_to_efi, &[])
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
        // Before QEMU 5.0.0, vvfat implementation has a bug that causes SEGV on exit
        // so not checking the exit status code here to avoid false-positives.
        // c.f. https://github.com/qemu/qemu/commit/8475ea48544b313cf533312846a4899ddecb799c
        eprintln!("QEMU exited with status: {status:?}");
        Ok(())
    }
    // No new lines in cmd is allowed (it will be added automatically)
    pub async fn send_monitor_cmd(&mut self, cmd: &str) -> Result<()> {
        let mut monitor = QemuMonitor::new(&self.monitor_sock_path()?).await?;
        monitor.send(cmd).await
    }
    pub async fn send_key_inputs_from_str(&mut self, s: &str) -> Result<()> {
        // https://gist.github.com/mvidner/8939289
        for c in s.chars() {
            let c = match c {
                '\n' => "ret".to_string(),
                ' ' => "spc".to_string(),
                '.' => "dot".to_string(),
                _ => c.to_string(),
            };
            self.send_monitor_cmd(&format!("sendkey {c}")).await?;
        }
        Ok(())
    }
    pub async fn kill(&mut self) -> Result<()> {
        if let Err(e) = self.send_monitor_cmd("quit\n").await {
            eprintln!("Qemu::kill : send_monitor_cmd returned an error but it is expected since the connection is lost: {e:?}")
        }
        self.wait_to_be_killed()
    }
}

impl Drop for Qemu {
    fn drop(&mut self) {
        if let Some(mut proc) = self.proc.take() {
            let pid = proc.id();
            if let Ok(Some(exit_status)) = proc.try_wait() {
                eprintln!("QEMU (pid = {pid}) is already exited at Drop. status: {exit_status:?}",);
            } else {
                // looks like the process is still running so kill it
                eprintln!("Sending kill signal to QEMU (pid = {})", proc.id(),);
                proc.kill()
                    .context(anyhow!("Failed to kill QEMU (id = {})", proc.id()))
                    .unwrap();
                eprintln!(
                    "QEMU (pid = {}) is killed since it is dropped. status: {:?}",
                    proc.id(),
                    proc.try_wait()
                );
            }
        }
    }
}
