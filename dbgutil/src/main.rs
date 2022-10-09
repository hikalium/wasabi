#![feature(io_read_to_string)]
use pdb::FallibleIterator;
use regex::Regex;
use rustc_demangle::demangle;
use std::collections::BTreeMap;
use std::fs::File;
use std::io;
use std::io::Read;
use std::ops::Bound;
use std::process::Command;

fn main() -> io::Result<()> {
    let find_pdb_path_output = Command::new("bash")
        .arg("-c")
        .arg("ls -1t target/x86_64-unknown-uefi/release/deps/os-*.pdb | head -1")
        .output()
        .expect("failed to execute objdump");
    println!("{:?}", find_pdb_path_output.stdout);
    let mut pdb_path = String::from_utf8(find_pdb_path_output.stdout)
        .unwrap()
        .to_string();
    let pdb_path = pdb_path.trim();
    println!("{}", pdb_path);

    println!("Paste console output here and press Ctrl-D:");
    let mut buf = vec![];
    io::stdin().read_to_end(&mut buf)?;
    let input = String::from_utf8_lossy(&buf);
    let lines: Vec<&str> = input.split('\n').collect();
    let re_loader_code = Regex::new(r"\[0x(.*)-0x(.*)\).*type: LOADER_CODE").unwrap();
    let re_qemu_exception_info = Regex::new(r"v=.* cpl=.* IP=.*:(.*) pc=.*").unwrap();
    let mmap_loader_code = lines
        .iter()
        .map(|&line| re_loader_code.captures(line))
        .find(|line| line.is_some())
        .expect("LOADER_CODE memory map entry is not found")
        .unwrap();
    let qemu_exception = lines
        .iter()
        .map(|&line| re_qemu_exception_info.captures(line))
        .find(|line| line.is_some())
        .expect("QEMU exception info not found")
        .unwrap();
    let loader_base =
        u64::from_str_radix(&mmap_loader_code[1], 16).expect("failed to parse LOADER_CODE base");
    let rip = u64::from_str_radix(&qemu_exception[1], 16).expect("failed to parse RIP");

    let re_objdump_section_text = Regex::new(r"([a-zA-Z0-9]+) <.text>").unwrap();
    let objdump_output = Command::new("objdump")
        .arg("-d")
        .arg("target/x86_64-unknown-uefi/debug/os.efi")
        .output()
        .expect("failed to execute objdump");
    let input = String::from_utf8(objdump_output.stdout).unwrap();
    let lines: Vec<&str> = input.split('\n').collect();
    let objdump_line_text = lines
        .iter()
        .map(|&line| re_objdump_section_text.captures(line))
        .find(|line| line.is_some())
        .expect(".text section not found")
        .unwrap();
    let text_base =
        u64::from_str_radix(&objdump_line_text[1], 16).expect("failed to parse text_base");
    let addr_in_text = rip - loader_base + text_base;
    println!("loader_base  ={:#018X}", loader_base);
    println!("RIP          ={:#018X}", rip);
    println!(".text base   ={:#018X}", text_base);
    println!("addr_in_text ={:#018X}", addr_in_text);

    let file = File::open(pdb_path)?;
    let mut pdb = pdb::PDB::open(file).unwrap();

    let symbol_table = pdb.global_symbols().unwrap();
    let address_map = pdb.address_map().unwrap();

    let mut sorted_symbols = BTreeMap::new();
    let mut symbols = symbol_table.iter();
    while let Some(symbol) = symbols.next().unwrap() {
        match symbol.parse() {
            Ok(pdb::SymbolData::Public(data)) if data.function => {
                // we found the location of a function!
                let rva = data.offset.to_rva(&address_map).unwrap_or_default();
                let addr_on_memory = rva.0 as u64 + loader_base;
                sorted_symbols.insert(addr_on_memory, demangle(&data.name.to_string()).to_string());
            }
            _ => {}
        }
    }
    let symbol_entry = sorted_symbols
        .range((Bound::Unbounded, Bound::Included(rip)))
        .next_back()
        .expect("Symbol not found");
    println!("{:#018X}: {}", symbol_entry.0, symbol_entry.1);

    Ok(())
}
