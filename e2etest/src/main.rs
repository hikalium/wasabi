#![feature(exit_status_error)]

use anyhow::Context;
use anyhow::Result;
use argh::FromArgs;
use e2etest::build_wasabi;
use e2etest::dump_dev_env;
use e2etest::spawn_shell_cmd_at_nocapture;
use futures::executor::block_on;
use std::io::Write;
use std::thread::sleep;
use std::time::Duration;

#[derive(FromArgs, PartialEq, Debug)]
/// WasabiOS end-to-end test runner
struct Args {
    /// absolute path for the WasabiOS project
    #[argh(option)]
    project_root: String,
}

fn test_qemu_is_killable_via_monitor() -> Result<()> {
    let mut child =
        spawn_shell_cmd_at_nocapture("qemu-system-x86_64 -monitor stdio -display none", ".")?;
    eprintln!("QEMU spawned");
    sleep(Duration::from_millis(1000));
    let mut stdin = child.stdin.take().context("stdin was None")?;
    stdin.write_all(b"quit\n")?;
    let status = child.wait()?;
    status
        .exit_ok()
        .context("QEMU should exit succesfully with quit command, but got error")?;
    eprintln!("QEMU exited succesfully");
    Ok(())
}

async fn run_tests() -> Result<()> {
    test_qemu_is_killable_via_monitor()?;
    Ok(())
}

fn main() -> Result<()> {
    let args: Args = argh::from_env();
    let project_root = args.project_root;
    println!("Project root  : {project_root}");

    std::env::set_current_dir(project_root)?;
    std::env::remove_var("RUSTUP_TOOLCHAIN");

    dump_dev_env()?;
    build_wasabi()?;
    block_on(async {
        run_tests().await?;
        Result::<()>::Ok(())
    })?;

    Ok(())
}
