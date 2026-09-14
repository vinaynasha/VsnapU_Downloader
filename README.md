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

**Note on the app icon:** `src-tauri/icons/icon.png` is currently a placeholder (a solid-color
256x256 PNG), added only because Tauri's `generate_context!()` macro requires *some* icon file to
exist at compile time. `tauri.conf.json`'s `bundle.icon` is intentionally left empty (`[]`) until
real branding art exists. Before building a real installer for distribution, generate a proper
icon set from real artwork with `cargo tauri icon <path-to-a-1024x1024-png>` (this produces the
full `.ico`/`.icns`/PNG set Tauri's bundler needs) and populate `bundle.icon` with the generated
paths -- otherwise the Windows MSI bundler in particular may fail or fall back to a default icon.

## Testing the protocol handler locally

1. Build and install the app once (`cargo tauri build`, then run the produced installer) so the
   OS registers it as the handler for `vsnapu-download://`.
2. Open a terminal and run: `open "vsnapu-download://fetch?manifest=<a-real-manifest-url>"` (Mac)
   or `start "vsnapu-download://fetch?manifest=<a-real-manifest-url>"` (Windows).
3. The app should come to the foreground and start downloading.
