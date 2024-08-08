#![feature(exit_status_error)]

use anyhow::Context;
use anyhow::Result;
use argh::FromArgs;
use lazy_static::lazy_static;
use pdb::FallibleIterator;
use pdb::ImageSectionHeader;
use pdb::SymbolData;
use regex::Regex;
use rustc_demangle::demangle;
use serde::Deserialize;
use serde::Serialize;
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
    static ref RE_LOADER_CODE: Regex = Regex::new(r"\[0x(.*)-0x(.*)\).*type: LOADER_CODE").unwrap();
    static ref RE_OBJDUMP_SECTION_TEXT: Regex =
        Regex::new(r"(?P<addr>[a-zA-Z0-9]+) <.text>").unwrap();
    static ref RE_DEBUG_METADATA: Regex =
        Regex::new(r"DEBUG_METADATA: print_kernel_debug_metadata = 0x(?P<addr>[a-fA-F0-9]+)$")
            .unwrap();
    static ref RE_DEBUG_INFO_ADDR_WRITE_CR3: Regex =
        Regex::new(r"^debug_info: write_cr3 = 0x(?P<addr>[a-fA-F0-9]+)$").unwrap();
}

struct CodeParams {
    text_ofs_to_runtime_addr: u64,
    sorted_symbol_names: BTreeMap<u64, String>,
    text_section: ImageSectionHeader,
    text_base_in_objdump: u64,
    objdump_lines: Vec<String>,
}
#[derive(Serialize, Deserialize)]
struct ExceptionInfo {
    count: u64,
    intno: u64,
    error_code: u64,
    is_int: u64,
    cpl: u64,
    cs_sel: u64,
    rip: u64,
    ss_sel: u64,
    rsp: u64,
    cr2: u64,
    rbp: u64,
    op_bytes: String,
}
fn dump_rip(rip: u64, params: &CodeParams) -> Result<()> {
    let text_ofs = rip - params.text_ofs_to_runtime_addr;
    let rip_in_objdump = text_ofs + params.text_base_in_objdump;
    let rip_in_objdump_str = format!("{rip_in_objdump:x}");

    if text_ofs < params.text_section.size_of_raw_data as u64 {
        let symbol_entry = params
            .sorted_symbol_names
            .range((Bound::Unbounded, Bound::Included(text_ofs)))
            .next_back()
            .expect("Symbol not found");
        println!("\n.text + {:#018X}: {}", symbol_entry.0, symbol_entry.1);
    }
    let lines: Vec<&String> = params
        .objdump_lines
        .iter()
        .skip_while(|s| !s.starts_with(&rip_in_objdump_str))
        .take(8)
        .collect();
    for line in lines {
        println!("{line}");
    }
    Ok(())
}

#[derive(Debug)]
struct DebugFiles {
    pdb_path: String,
    efi_path: String,
}

fn get_latest_file(pattern: &str) -> Result<String> {
    let result = Command::new("bash")
        .arg("-c")
        .arg(format!("ls -1t {} | head -1", pattern))
        .output()
        .expect("failed to execute ls");
    result.status.exit_ok()?;
    let result = String::from_utf8(result.stdout)?;
    let result = result.as_str().trim().to_string();
    Ok(result)
}

fn detect_files() -> Result<DebugFiles> {
    let pdb_path = get_latest_file("target/x86_64-unknown-uefi/*/deps/os-*.pdb")
        .context("failed to detect pdb_path")?;
    let efi_path = get_latest_file("target/x86_64-unknown-uefi/*/os.efi")
        .context("failed to detect efi_path")?;
    Ok(DebugFiles { pdb_path, efi_path })
}

fn main() -> Result<()> {
    let args: Args = argh::from_env();

    let files = detect_files()?;
    println!("{:?}", files);
    let file = File::open(files.pdb_path)?;
    let mut pdb = pdb::PDB::open(file).unwrap();
    let sections = pdb.sections().unwrap().unwrap();
    println!(
        "{:8} {:10} {:10} {:10} {:10} {:10}",
        "text", "paddr", "vaddr", "raw_ofs", "raw_size", "raw_ofs_end"
    );
    for s in &sections {
        println!(
            "{:8} {:#010X} {:#010X} {:#010X} {:#010X} {:#010X}",
            s.name(),
            s.physical_address,
            s.virtual_address,
            s.pointer_to_raw_data,
            s.size_of_raw_data,
            s.pointer_to_raw_data + s.size_of_raw_data,
        );
    }
    let text_section = *sections.iter().find(|s| s.name() == ".text").unwrap();
    let symbols = pdb.global_symbols().unwrap();
    let mut symbols = symbols.iter();
    let mut sorted_symbols = BTreeMap::new();
    let mut sorted_symbol_names = BTreeMap::new();
    let mut symbol_table = HashMap::<String, SymbolData>::new();
    let mut symbol_text_ofs_table = HashMap::<String, u64>::new();
    {
        while let Some(symbol) = symbols.next().unwrap() {
            let symbol = symbol.parse()?;
            match symbol {
                pdb::SymbolData::Public(data) if data.function => {
                    let ofs_in_section = data.offset.offset as u64;
                    let name = demangle(&data.name.to_string()).to_string();
                    sorted_symbol_names.insert(ofs_in_section, name.clone());
                    sorted_symbols.insert(ofs_in_section, data);
                    symbol_table.insert(name.clone(), symbol);
                    symbol_text_ofs_table.insert(name, ofs_in_section);
                }
                _ => {}
            }
        }
    }

    let objdump_lines = Command::new("objdump")
        .arg("-d")
        .arg(&files.efi_path)
        .output()
        .expect("failed to execute objdump");
    let objdump_lines = String::from_utf8(objdump_lines.stdout).unwrap();
    let objdump_lines: Vec<String> = objdump_lines.split('\n').map(|s| s.to_string()).collect();

    let text_base_in_objdump = objdump_lines
        .iter()
        .find_map(|line| RE_OBJDUMP_SECTION_TEXT.captures(line))
        .unwrap();
    let text_base_in_objdump = u64::from_str_radix(&text_base_in_objdump["addr"], 16)
        .expect("failed to parse objdump for text section");

    match args.nested {
        SubCommand::Crash(args) => {
            let serial_log = std::fs::read_to_string(args.serial_log)?;
            let serial_log: Vec<&str> = serial_log.trim().split('\n').collect();

            let text_ofs_to_runtime_addr = {
                let efi_main_runtime_addr = serial_log
                    .iter()
                    .find_map(|line| RE_DEBUG_METADATA.captures(line))
                    .expect("No log message that matches with RE_DEBUG_METADATA found");
                let efi_main_runtime_addr = u64::from_str_radix(&efi_main_runtime_addr["addr"], 16)
                    .expect("failed to parse efi_main_runtime_addr");
                let efi_main_text_ofs = symbol_text_ofs_table
                    .get("print_kernel_debug_metadata")
                    .context("print_kernel_debug_metadata not found in symbol_text_ofs_table")?;
                efi_main_runtime_addr - efi_main_text_ofs
            };
            eprintln!("text_ofs_to_runtime_addr = {text_ofs_to_runtime_addr:#X}");

            // *_runtime_addr: actual address (e.g. RIP values) at runtime
            // *_text_ofs: offset in the .text section
            // so that:
            // *_runtime_addr = *_text_ofs + text_ofs_to_runtime_addr
            let params = CodeParams {
                text_ofs_to_runtime_addr,
                sorted_symbol_names,
                text_section,
                text_base_in_objdump,
                objdump_lines,
            };
            if let Some(phys_addr) = args.phys_addr {
                let rip = u64::from_str_radix(&phys_addr, 16).expect("failed to parse RIP");
                return dump_rip(rip, &params);
            }
            if let Some(qemu_debug_log) = args.qemu_debug_log {
                let qemu_log = std::fs::read_to_string(qemu_debug_log)?;
                for exception_info in qemu_log
                    .split('\n')
                    .flat_map(|s| s.strip_prefix("hikalium_exception:"))
                    .flat_map(serde_json::from_str::<ExceptionInfo>)
                {
                    let rip = exception_info
                        .rip
                        .to_string()
                        .parse::<u64>()
                        .expect("failed to parse RIP");
                    if exception_info.intno >= 0x20 {
                        // Not a CPU Exception
                        continue;
                    }
                    println!(
                        "\nException {:#04X} @ {:#018X}: op_bytes = {}",
                        &exception_info.intno, rip, exception_info.op_bytes
                    );
                    dump_rip(rip, &params)?;
                }
            }
        }
        SubCommand::Symbol(_) => {
            for sym in sorted_symbols {
                println!("{:#018X} : {:?}", sym.0, sym.1)
            }
        }
    }

    Ok(())
}
