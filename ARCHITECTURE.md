# Architecture

## Protocol handoff

The web app (`apps.vsnapu.com`) mints a short-lived manifest link via
`GET /api/DirectDownload/CreateManifestLink` on the VSnapU backend, then opens:

```
vsnapu-download://fetch?manifest=<url-encoded manifest link>
```

The OS routes this to the installed app via `tauri-plugin-deep-link`, which the app never has to
poll for -- registered at install time.

## Manifest contract

`GET <manifest link>` (itself `GET /api/DirectDownload/Manifest?token=...` on the VSnapU backend)
returns:

```json
{
  "jobName": "string",
  "folderName": "string | null",
  "files": [
    { "fileName": "IMG_0001.jpg", "url": "https://...", "sizeBytes": 8421000 }
  ]
}
```

Each file's `url` is independently downloadable with no further auth -- either a direct signed
GCS URL, or a signed link back to the VSnapU backend's `/api/DirectDownload/File` streaming
proxy for files still on the server's local drives. Both kinds are short-lived and self-contained
(the app never authenticates on its own -- see the VSnapU repo's
`docs/superpowers/specs/2026-08-21-desktop-downloader-app-design.md`, Decision 7).

## Download behavior

- Fixed concurrency of 4 simultaneous file downloads (`src-tauri/src/download.rs`).
- Each file streams straight to its final destination path (chosen by the user on first run,
  remembered via `tauri-plugin-store`) -- no zip, no temp merge step.
- HTTP Range requests resume a partially-downloaded file rather than restarting it.
- Progress is reported per-file and as an overall job total, using the manifest's `sizeBytes`.

## Signing and notarization

Explicitly deferred. Builds are unsigned during development and internal testing; a Windows
Authenticode certificate and an Apple Developer account are a separate decision to make before
any public/client-facing distribution.
