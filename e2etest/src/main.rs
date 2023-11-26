#![feature(exit_status_error)]

use anyhow::Result;
use argh::FromArgs;
use e2etest::build_wasabi;
use e2etest::dump_dev_env;
use e2etest::qemu::Qemu;
use std::thread::sleep;
use std::time::Duration;

#[derive(FromArgs, PartialEq, Debug)]
/// WasabiOS end-to-end test runner
struct Args {
    /// absolute path for the WasabiOS project
    #[argh(option)]
    project_root: String,
}

async fn test_qemu_is_killable_via_monitor() -> Result<()> {
    let mut qemu = Qemu::launch_without_os()?;
    sleep(Duration::from_millis(500));
    qemu.kill().await?;
    Ok(())
}

async fn run_tests() -> Result<()> {
    test_qemu_is_killable_via_monitor().await?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Args = argh::from_env();
    let project_root = args.project_root;
    println!("Project root  : {project_root}");

    std::env::set_current_dir(project_root)?;
    std::env::remove_var("RUSTUP_TOOLCHAIN");

    dump_dev_env()?;
    build_wasabi()?;
    run_tests().await?;
    Ok(())
}
