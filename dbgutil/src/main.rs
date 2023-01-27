use anyhow::Context;
use anyhow::Result;
use argh::FromArgs;
use lazy_static::lazy_static;
use pdb::FallibleIterator;
use regex::Regex;
use rustc_demangle::demangle;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::fs::File;
use std::ops::Bound;
use std::process::Command;

#[derive(FromArgs, PartialEq, Debug)]
/// Wasabi OS Debug Tool
struct Args {
    #[argh(subcommand)]
    nested: SubCommand,
}

#[derive(FromArgs, PartialEq, Debug)]
#[argh(subcommand)]
enum SubCommand {
    Crash(CrashArgs),
    Symbol(SymbolArgs),
}

#[derive(FromArgs, PartialEq, Debug)]
/// analyze crash
#[argh(subcommand, name = "crash")]
struct CrashArgs {
    #[argh(option)]
    /// path to qemu_debug.log
    qemu_debug_log: Option<String>,
    #[argh(option)]
    /// path to serial console output
    serial_log: String,
    #[argh(option)]
    /// phys addr to check
    phys_addr: Option<String>,
}

#[derive(FromArgs, PartialEq, Debug)]
/// dump symbols
#[argh(subcommand, name = "symbol")]
struct SymbolArgs {}

lazy_static! {
    static ref RE: Regex = Regex::new("...").unwrap();
    static ref RE_LOADER_CODE: Regex = Regex::new(r"\[0x(.*)-0x(.*)\).*type: LOADER_CODE").unwrap();
    static ref RE_QEMU_EXCEPTION_INFO: Regex =
        Regex::new(r"IP=[0-9a-fA-F]+:([0-9a-fA-F]+)").unwrap();
    static ref RE_OBJDUMP_SECTION_TEXT: Regex = Regex::new(r"([a-zA-Z0-9]+) <.text>").unwrap();
    static ref RE_WASABI_BOOTED: Regex =
        Regex::new(r"^Wasabi OS booted\. efi_main = 0x([a-fA-F0-9]+)$").unwrap();
}

fn main() -> Result<()> {
    let args: Args = argh::from_env();

    let pdb_path = Command::new("bash")
        .arg("-c")
        .arg("ls -1t target/x86_64-unknown-uefi/debug/deps/os-*.pdb | head -1")
        .output()
        .expect("failed to execute objdump");
    let pdb_path = String::from_utf8(pdb_path.stdout)?;
    let pdb_path = pdb_path.as_str().trim();
    println!("pdb_path: {}", pdb_path);

    let efi_path = "target/x86_64-unknown-uefi/debug/os.efi";
    println!("efi_path: {}", efi_path);

    let file = File::open(pdb_path)?;
    let mut pdb = pdb::PDB::open(file).unwrap();
    if let Ok(Some(sections)) = pdb.sections() {
        for s in sections {
            eprintln!("{s:?}");
        }
    }
    let symbols = pdb.global_symbols().unwrap();
    let mut symbols = symbols.iter();
    let address_map = pdb.address_map().unwrap();
    let mut sorted_symbols = BTreeMap::new();
    let mut symbol_table = HashMap::<String, u64>::new();
    while let Some(symbol) = symbols.next().unwrap() {
        match symbol.parse() {
            Ok(pdb::SymbolData::Public(data)) if data.function => {
                // we found the location of a function!
                let rva = data.offset.to_rva(&address_map).unwrap_or_default();
                let ofs_in_file = rva.0 as u64;
                let name = demangle(&data.name.to_string()).to_string();
                sorted_symbols.insert(ofs_in_file, name.clone());
                symbol_table.insert(name, ofs_in_file);
            }
            _ => {}
        }
    }

    let objdump_lines = Command::new("objdump")
        .arg("-d")
        .arg(efi_path)
        .output()
        .expect("failed to execute objdump");
    let objdump_lines = String::from_utf8(objdump_lines.stdout).unwrap();
    let objdump_lines: Vec<&str> = objdump_lines.split('\n').collect();
    let objdump_line_text = objdump_lines
        .iter()
        .map(|&line| RE_OBJDUMP_SECTION_TEXT.captures(line))
        .find(|line| line.is_some())
        .expect(".text section not found")
        .unwrap();

    match args.nested {
        SubCommand::Crash(args) => {
            let serial_log = std::fs::read_to_string(args.serial_log)?;
            let serial_log: Vec<&str> = serial_log.trim().split('\n').collect();

            let efi_main_runtime_addr = serial_log
                .iter()
                .find_map(|line| RE_WASABI_BOOTED.captures(line))
                .expect("'Wasabi OS booted' message is not found");
            let efi_main_runtime_addr = u64::from_str_radix(&efi_main_runtime_addr[1], 16)
                .expect("failed to parse efi_main_runtime_addr");
            println!("efi_main_runtime_addr = 0x{:018X}", efi_main_runtime_addr);

            let efi_main_file_offset = symbol_table
                .get("efi_main")
                .context("efi_main not found in symbol_table")?;
            println!("efi_main_file_offset = 0x{:018X}", efi_main_file_offset);

            if let Some(phys_addr) = args.phys_addr {
                let rip = u64::from_str_radix(&phys_addr, 16).expect("failed to parse RIP");

                let rip_file_offset = rip - efi_main_runtime_addr + efi_main_file_offset;
                println!("rip_file_offset ={:#018X}", rip_file_offset);
                println!("RIP             ={:#018X}", rip);

                let symbol_entry = sorted_symbols
                    .range((Bound::Unbounded, Bound::Included(rip_file_offset)))
                    .next_back()
                    .expect("Symbol not found");
                println!("{:#018X}: {}", symbol_entry.0, symbol_entry.1);
                return Ok(());
            }

            if let Some(qemu_debug_log) = args.qemu_debug_log {
                let qemu_debug_log = std::fs::read_to_string(qemu_debug_log)?;
                for qemu_exception in RE_QEMU_EXCEPTION_INFO.captures_iter(&qemu_debug_log) {
                    let rip =
                        u64::from_str_radix(&qemu_exception[1], 16).expect("failed to parse RIP");

                    let rip_file_offset = rip - efi_main_runtime_addr + efi_main_file_offset;
                    println!("rip_file_offset ={:#018X}", rip_file_offset);
                    println!("RIP             ={:#018X}", rip);

                    let symbol_entry = sorted_symbols
                        .range((Bound::Unbounded, Bound::Included(rip_file_offset)))
                        .next_back()
                        .expect("Symbol not found");
                    println!("{:#018X}: {}", symbol_entry.0, symbol_entry.1);
                }
            }
        }
        SubCommand::Symbol(_) => {
            for sym in sorted_symbols {
                println!("{:#018X} : {}", sym.0, sym.1)
            }
        }
    }

    Ok(())
}
