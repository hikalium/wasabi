// Compile-time helpers that look up data about the build environment
// and expand to plain Rust literals usable in `no_std` callers.
//
// `git_hash!()` runs `git rev-parse --short HEAD` while this crate is
// being compiled and yields a `&'static str` literal containing the
// short commit hash, with `-dirty` appended if the working tree has
// uncommitted changes. Falls back to `"unknown"` when `git` isn't
// available or the project isn't a git checkout.

use proc_macro::TokenStream;
use std::process::Command;

#[proc_macro]
pub fn git_hash(_input: TokenStream) -> TokenStream {
    let hash = run_git(&["rev-parse", "--short", "HEAD"])
        .unwrap_or_else(|| "unknown".to_string());
    let suffix = if is_dirty() { "-dirty" } else { "" };
    str_literal(&format!("{hash}{suffix}"))
}

fn str_literal(value: &str) -> TokenStream {
    // Debug formatting escapes any unexpected character into a valid
    // Rust string literal.
    let lit = format!("{value:?}");
    lit.parse().expect("invalid str literal produced")
}

fn run_git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8(out.stdout).ok()?;
    Some(s.trim().to_string())
}

fn is_dirty() -> bool {
    Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .map(|o| o.status.success() && !o.stdout.is_empty())
        .unwrap_or(false)
}
