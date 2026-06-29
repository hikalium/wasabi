# CLAUDE.md

## Mutex pitfalls

`crate::mutex::Mutex::lock()` spins ~10000 times then panics with the
message `Failed to lock Mutex at <created_at_file>:<created_at_line>`.
The cited line is the **creation site** of the mutex, not the offending
caller — when chasing a "failed to lock" panic, grep for the static or
field at that file:line and look at the *callers*.

The most common way to trip this is a self-deadlock through an `if let`
or `match` temporary:

```rust
// BUG: the MutexGuard temporary lives for the whole if-let body.
if let Some(p) = SOME_MUTEX.lock().as_ref() {
    // ...
    *SOME_MUTEX.lock() = None;   // <- spins forever, panics
}
```

Fix by extracting what you need under a short-lived guard, dropping it,
then re-locking:

```rust
let copy = SOME_MUTEX.lock().as_ref().map(|p| (p.a, p.b));
if let Some((a, b)) = copy {
    // ...
    *SOME_MUTEX.lock() = None;   // <- previous guard dropped, OK
}
```

Same shape applies to `match SOME_MUTEX.lock().as_ref() { ... }` and to
`for x in COLLECTION_MUTEX.lock().iter() { ... }`. If the body needs to
take the same lock — even transitively, e.g. by calling a helper that
locks it — copy out first.
