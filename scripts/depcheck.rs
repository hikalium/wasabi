#!/usr/bin/env -S cargo -Zscript
//! ```cargo
//! [dependencies]
//! anyhow = "1.0.86"
//! ```

// rustup install nightly

use anyhow::Result;
use std::collections::HashSet;
use std::fs::File;
use std::io::Write;
use std::process::Command;
use std::str;

fn get_git_root_path() -> Result<String> {
    let result = Command::new("bash")
        .arg("-c")
        .arg(format!("git rev-parse --show-toplevel"))
        .output()
        .unwrap()
        .stdout;
    let git_root_path = str::from_utf8(&result)?.trim();
    eprintln!("git_root_path: {git_root_path}");
    Ok(git_root_path.to_string())
}

// <https://doc.rust-lang.org/book/ch07-02-defining-modules-to-control-scope-and-privacy.html>
fn get_paths_to_the_code_per_files(git_root_path: &str) -> Result<HashSet<(String, String)>> {
    let result = Command::new("bash")
        .arg("-c")
        .arg(format!(
            r"git grep -o '[A-Za-z0-9_]*::[A-Za-z0-9_:*]*' *.rs | sed 's/:/ /'"
        ))
        .current_dir(&git_root_path)
        .output()
        .unwrap()
        .stdout;
    let grep_output = str::from_utf8(&result)?.trim();
    let mut depset: HashSet<(String, String)> = grep_output
        .split("\n")
        .map(|s| {
            let s: Vec<&str> = s.trim().split(" ").collect();
            let mut s = s.into_iter();
            (
                s.next().unwrap_or_default().to_string(),
                s.next().unwrap_or_default().to_string(),
            )
        })
        .collect();
    Ok(depset)
}
fn get_struct_set(git_root_path: &str) -> Result<HashSet<String>> {
    let result = Command::new("bash")
        .arg("-c")
        .arg(format!(
            r"git grep -o 'struct [A-Z][A-Za-z0-9]* ' | sed -e 's#/src/#::#' -e 's#main\.rs:##' -e 's#lib\.rs:##' -e 's#struct ##' -e 's/\.rs:/::/' -e 's#/#::#' | sort -u"
        ))
        .current_dir(git_root_path)
        .output()
        .unwrap()
        .stdout;
    let grep_output = str::from_utf8(&result)?.trim();
    let struct_set: HashSet<String> = grep_output.split("\n").map(|s| s.to_string()).collect();
    Ok(struct_set)
}
fn get_per_file_module_deps(
    git_root_path: &str,
    struct_set: &HashSet<String>,
) -> Result<HashSet<(String, String)>> {
    let mut mod_dep_set: HashSet<(String, String)> = HashSet::new();
    let mut mod_set = HashSet::<String>::new();
    for s in struct_set {
        let ss: Vec<&str> = s.split("::").collect();
        let mut prev = s.to_string();
        for t in (1..ss.len()).rev() {
            let t = ss[0..t].join("::");
            mod_set.insert(t.clone());
            mod_dep_set.insert((t.clone(), prev));
            prev = t;
        }
    }
    Ok(mod_dep_set)
}

fn main() -> Result<()> {
    let git_root_path = get_git_root_path()?;
    let depset = get_paths_to_the_code_per_files(&git_root_path)?;
    let struct_set = get_struct_set(&git_root_path)?;
    let mod_dep_set = get_per_file_module_deps(&git_root_path, &struct_set)?;
    //
    eprintln!("{struct_set:?}");
    return Ok(());

    let fset: HashSet<&str> = depset.iter().map(|e| e.0.as_str()).collect();
    let vset: HashSet<&str> = depset.iter().map(|e| e.1.as_str()).collect();
    let mut f = File::create("dep.dot")?;
    writeln!(f, "digraph DEP {{")?;
    writeln!(f, "layout=dot")?;
    writeln!(f, "overlap=false")?;
    writeln!(f, "rankdir=LR")?;
    for (from, to) in mod_dep_set.iter() {
        writeln!(f, r#"{from:?} -> {to:?};"#)?;
    }
    for (i, k) in struct_set.iter().enumerate() {
        let y = i;
        writeln!(
            f,
            r#"{k:?} [shape="box", pin=false,pos="0,{y}", fontname="monospace"];"#
        )?;
    }
    for (i, k) in mod_dep_set.iter().enumerate() {
        let y = i;
        writeln!(
            f,
            r#"{k:?} [shape="box", pin=false,pos="0,{y}", fontname="monospace"];"#
        )?;
    }
    for (k, v) in &depset {
        let lastpart = v.split("::").last().unwrap_or_default();
        if matches!(lastpart.find(char::is_uppercase), Some(0)) {
            eprintln!("{v} -> {k};");
            writeln!(f, r#"{v:?} -> {k:?};"#)?;
        }
    }

    writeln!(f, "}}")?;
    Ok(())
}
