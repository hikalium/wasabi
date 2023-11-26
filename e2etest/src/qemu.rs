use crate::spawn_shell_cmd_at_nocapture;
use anyhow::anyhow;
use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use std::io;
use std::process;
use std::process::ExitStatus;
use std::thread::sleep;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::net::UnixStream;

pub struct Qemu {
    proc: process::Child,
}

impl Qemu {
    const MONITOR_SOCK: &str = "qemu_monitor.sock";
    const COMMON_ARGS: &str = "-d int,cpu_reset -D log/qemu_debug.txt";
    pub fn launch_without_os() -> Result<Self> {
        let proc = spawn_shell_cmd_at_nocapture(
            &format!(
                "qemu-system-x86_64 -monitor unix:{},server,nowait -display none {}",
                Self::MONITOR_SOCK,
                Self::COMMON_ARGS
            ),
            ".",
        )?;
        eprintln!("QEMU spawned: id = {}", proc.id());
        Ok(Self { proc })
    }
    fn wait_to_be_killed(&mut self) -> Result<()> {
        const TIMEOUT: Duration = Duration::from_secs(100);
        let mut duration = Duration::ZERO;
        let interval = Duration::from_millis(500);
        let status: ExitStatus = loop {
            let status = self.proc.try_wait()?;
            if let Some(status) = status {
                break Result::<ExitStatus>::Ok(status);
            }
            sleep(interval);
            duration += interval;
            if duration > TIMEOUT {
                bail!("Waiting too long to kill ({TIMEOUT:?})");
            }
            eprintln!("Waiting QEMU to be killed...")
        }?;
        status
            .exit_ok()
            .context("QEMU should exit succesfully with quit command, but got error")?;
        eprintln!("QEMU exited succesfully");
        Ok(())
    }
    async fn send_monitor_cmd(&mut self, cmd: &str) -> Result<()> {
        let mut stream = UnixStream::connect(Self::MONITOR_SOCK).await?;
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
                    print!("{}", &String::from_utf8_lossy(&buf));
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
        if self.proc.try_wait().is_err() {
            // looks like the process is still running so kill it
            self.proc
                .kill()
                .context(anyhow!("Failed to kill QEMU (id = {})", self.proc.id()))
                .unwrap();
        }
    }
}
