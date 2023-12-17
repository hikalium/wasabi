use crate::get_first_line;
use crate::run_shell_cmd;
use crate::run_shell_cmd_at_nocapture;
use anyhow::Result;
use serde::Deserialize;
use serde::Serialize;
use std::path::PathBuf;

/// Subset of `cargo metadata --format-version 1`
#[derive(Serialize, Deserialize, Debug)]
pub struct CargoMetadataV1 {
    /// Full-path of the target directory
    target_directory: String,
    /// Full-path of the workspace directory
    workspace_root: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CargoLocateOutput {
    /// Full-path of the workspace root's Cargo.toml
    root: String,
}

#[derive(Debug)]
pub struct DevEnv {
    wasabi_workspace_root_dir: String,
    wasabi_efi_path: String,
    ovmf_path: String,
}
impl DevEnv {
    /// A setup function to be called at the beginning of every test cases.
    pub fn new() -> Result<Self> {
        std::env::remove_var("RUSTUP_TOOLCHAIN");
        let (qemu_path, _) = run_shell_cmd("which qemu-system-x86_64")?;
        println!("QEMU path : {qemu_path}");
        let qemu_version = get_first_line(&run_shell_cmd(&format!("{qemu_path} --version"))?.0);
        println!("QEMU version  : {qemu_version}");

        let (make_path, _) = run_shell_cmd("which make")?;
        println!("Make path : {make_path}");
        let make_version = get_first_line(&run_shell_cmd(&format!("{make_path} --version"))?.0);
        println!("Make version  : {make_version}");

        let rust_toolchain = get_first_line(&run_shell_cmd("rustup show active-toolchain")?.0);
        println!("Rust version  : {rust_toolchain}");

        let cargo_workspace_root = &run_shell_cmd("cargo locate-project --workspace")?.0;
        let cargo_workspace_root: CargoLocateOutput =
            serde_json::from_str(cargo_workspace_root.as_str())?;
        let cargo_workspace_root = cargo_workspace_root.root;
        eprintln!("{cargo_workspace_root}");

        let cargo_metadata = &run_shell_cmd(&format!(
            "cargo metadata --format-version 1 --manifest-path {cargo_workspace_root}"
        ))?
        .0;
        let cargo_metadata: CargoMetadataV1 = serde_json::from_str(cargo_metadata.as_str())?;
        println!("Cargo metadata  : {cargo_metadata:?}");
        let wasabi_workspace_root_dir = cargo_metadata.workspace_root.to_string();
        let cargo_target_dir = cargo_metadata.target_directory;

        let mut efi_path = PathBuf::from(&cargo_target_dir);
        efi_path.push("x86_64-unknown-uefi");
        efi_path.push("release");
        efi_path.push("os.efi");
        let wasabi_efi_path = efi_path.to_string_lossy().to_string();

        println!("Entering dir : {wasabi_workspace_root_dir}");
        std::env::set_current_dir(&wasabi_workspace_root_dir)?;

        Self::build_wasabi()?;

        Ok(Self {
            wasabi_workspace_root_dir: wasabi_workspace_root_dir.clone(),
            wasabi_efi_path,
            ovmf_path: format!("{wasabi_workspace_root_dir}/third_party/ovmf/RELEASEX64_OVMF.fd"),
        })
    }
    pub fn wasabi_efi_path(&self) -> &str {
        &self.wasabi_efi_path
    }
    pub fn wasabi_workspace_root_dir(&self) -> &str {
        &self.wasabi_workspace_root_dir
    }
    pub fn ovmf_path(&self) -> &str {
        &self.ovmf_path
    }
    fn build_wasabi() -> Result<()> {
        run_shell_cmd_at_nocapture("cargo build --release", "./os/")
    }
}
