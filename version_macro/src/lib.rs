// Compile-time helpers that look up data about the build environment
// and expand to plain Rust literals usable in `no_std` callers.
//
// `git_hash!()` runs `git rev-parse --short HEAD` while this crate is
// being compiled and yields a `&'static str` literal containing the
// short commit hash, with `-dirty` appended if the working tree has
// uncommitted changes. Falls back to `"unknown"` when `git` isn't
// available or the project isn't a git checkout.

#![feature(track_path)]

use proc_macro::tracked_path;
use proc_macro::TokenStream;
use std::path::PathBuf;
use std::process::Command;

#[proc_macro]
pub fn git_hash(_input: TokenStream) -> TokenStream {
    // Tell rustc that this expansion's output depends on the working
    // tree's git state. Without this, incremental compilation reuses
    // the cached expansion (the macro takes no input tokens) even
    // after commits, branch switches, or unstaged edits. With it, any
    // change under one of the tracked paths invalidates the cache and
    // re-runs the macro on the next build.
    register_git_dependencies();

    let hash = run_git(&["rev-parse", "--short", "HEAD"])
        .unwrap_or_else(|| "unknown".to_string());
    let suffix = if is_dirty() { "-dirty" } else { "" };
    str_literal(&format!("{hash}{suffix}"))
}

fn register_git_dependencies() {
    let Some(git_dir) = run_git(&["rev-parse", "--git-dir"]).map(PathBuf::from)
    else {
        return;
    };
    // .git/HEAD: branch ref pointer or detached HEAD.
    track_if_exists(git_dir.join("HEAD"));
    // .git/index: anything `git add` touches.
    track_if_exists(git_dir.join("index"));
    // The current branch's tip ref. Walk HEAD ourselves to find it,
    // because the path varies with the current branch.
    if let Some(head_ref) = std::fs::read_to_string(git_dir.join("HEAD"))
        .ok()
        .and_then(|s| s.strip_prefix("ref: ").map(|r| r.trim().to_string()))
    {
        track_if_exists(git_dir.join(head_ref));
    }
    // Also track every working-tree file `git status` would consider —
    // that's exactly what flips the dirty marker. Cheap to enumerate
    // via `git ls-files`.
    if let Some(files) = run_git(&["ls-files"]) {
        let toplevel = run_git(&["rev-parse", "--show-toplevel"])
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        for rel in files.lines() {
            track_if_exists(toplevel.join(rel));
        }
    }
}

fn track_if_exists(p: PathBuf) {
    if p.exists() {
        if let Some(s) = p.to_str() {
            tracked_path::path(s);
        }
    }
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
