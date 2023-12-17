# e2etest for WasabiOS

## Run all tests

```
cargo test --bin e2etest
```

## Run some tests only

```
# run tests that matches the given filter 'bootable' for example
cargo test --bin e2etest -- bootable

# ...with real-time log messages
cargo test --bin e2etest -- bootable --nocapture
```
