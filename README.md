# VSnapU Downloader

A small Tauri desktop app (Windows + Mac) that downloads a VSnapU job's files directly and in
parallel, with no server-side zip step. Launched from the "Download with App" button on
`apps.vsnapu.com/download_new/...`.

## Dev setup

- Install Rust: https://rustup.rs
- Install the Tauri CLI: `cargo install tauri-cli --version "^2.0" --locked`
- Run in dev mode: `cargo tauri dev`

## Building installers

```bash
cargo tauri build
```

Produces a `.msi` (Windows) or `.dmg` (Mac) under `src-tauri/target/release/bundle/`, depending
on the host OS you build from. Builds are currently unsigned (see ARCHITECTURE.md) -- expect and
accept the OS "unrecognized publisher" warning during this phase.

## Testing the protocol handler locally

1. Build and install the app once (`cargo tauri build`, then run the produced installer) so the
   OS registers it as the handler for `vsnapu-download://`.
2. Open a terminal and run: `open "vsnapu-download://fetch?manifest=<a-real-manifest-url>"` (Mac)
   or `start "vsnapu-download://fetch?manifest=<a-real-manifest-url>"` (Windows).
3. The app should come to the foreground and start downloading.
