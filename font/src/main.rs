use std::env;
use std::fs::File;
use std::io::prelude::*;

fn main() -> std::io::Result<()> {
    let mut fonts: [[u8; 16]; 256] = [[0; 16]; 256];
    let args: Vec<String> = env::args().collect();
    let mut file = File::open(&args[1])?;
    let mut input = String::new();
    file.read_to_string(&mut input)?;
    let mut font_index = 0;
    let mut row_index = 16;
    let mut line = 0;
    for row in input.split('\n') {
        line += 1;
        if row.starts_with("0x") {
            assert!(
                row_index == 16,
                "line {}: fonts[0x{:02X}] has {} rows but expected 16",
                line,
                font_index,
                row_index
            );
            let row_trimmed = row.trim_start_matches("0x");
            font_index = match usize::from_str_radix(row_trimmed, 16) {
                Ok(i) => i,
                Err(_) => panic!("Failed to parse index line at line {}", line),
            };
            row_index = 0;
            continue;
        }
        if !row.starts_with('.') && !row.starts_with('*') {
            // skip blank lines
            continue;
        }
        assert!(
            row_index < 16,
            "line {}: fonts[0x{:02X}] has extra rows",
            line,
            font_index
        );
        let mut row_bits = 0;
        for i in 0..8 {
            match row.chars().nth(i) {
                Some('.') => (),
                Some('*') => row_bits |= 1 << i,
                c => panic!(
                    "line {}: fonts[0x{:02X}] has an unexpected character {:?}",
                    line, font_index, c
                ),
            }
        }
        fonts[font_index][row_index] = row_bits;
        row_index += 1;
    }

    println!("pub static BITMAP_FONT: [[u8; 16]; 256] = [");
    for f in fonts {
        print!("[");
        for (i, bits) in f.iter().enumerate() {
            print!("{}", bits);
            if i != 15 {
                print!(", ");
            }
        }
        println!("],");
    }
    println!("];");
    Ok(())
}
