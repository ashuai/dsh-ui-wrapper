# DSH — Minimal Shell for the dsh Web UI (macOS)

[![Rust CI](https://github.com/ashuai/dsh-ui-wrapper/actions/workflows/rust.yml/badge.svg)](https://github.com/ashuai/dsh-ui-wrapper/actions/workflows/rust.yml)

> English | [中文版](README.zh.md)

A minimal native macOS window that opens the dsh web UI at
`http://127.0.0.1:3080` using the **system browser engine (WebKit / WKWebView)**.
No custom UI, no browser chrome, no tabs — just the dsh GUI in its own window,
with a small backend auto-start helper so the page is there when you open it.

## Features

- Native window with the system WebKit engine (same engine as Safari, no WebView wrapper of our own, zero frontend code shipped)
- **Backend auto-start**: probes `127.0.0.1:3080` on launch; if it's down, starts dsh automatically (`bunx` → `pnpm` → `npm` → `dsh`), shows progress toasts (`Opening…` / `Starting… Ns` / `Ready`), and loads the page when the port is up
- **Fast-fail**: if the started process dies early (port taken, missing deps), an error panel with the backend log tail appears in ~0.4s instead of waiting for a timeout
- Adaptive light/dark boot page (follows system appearance)
- `Cmd+R` reload (handy after a backend restart); closing the window quits
- DeepSeek whale app icon (official 1024px asset; icon copyright belongs to DeepSeek)

## Requirements

- macOS 12+
- dsh backend: optional. If `127.0.0.1:3080` is already up, the app just loads it.
  Otherwise it auto-starts dsh — which needs one of `bunx` / `pnpm` / `npm` (or a global `dsh`)
  available on PATH or in common install dirs.

## Quick start

```bash
cd DSH
./make_app.sh                   # build + generate icon + assemble target/DSH.app
open target/DSH.app
```

Or run the raw binary: `./target/release/DSH`.

## How it works

```
Launch
 ├─ Show boot page (whale logo, adaptive theme, toast area)
 ├─ Background thread:
 │    ① probe 127.0.0.1:3080 (TCP, ~300ms)
 │       ├─ up        → toast "Ready" → load http://127.0.0.1:3080
 │       └─ down      → find runner (bunx/pnpm/npm/dsh, OS-aware) → spawn `… dsh web`
 │                      (detached, logs → ~/Library/Logs/DSH-backend.log)
 │                      poll every 400ms, toast ticks "Starting… Ns"
 │                      ├─ process died early → error panel (fast-fail)
 │                      └─ timeout (default 30s) → error panel + Retry
 └─ Ready → load the real page
```

The spawned dsh keeps running after the app quits (detached), so reopening is instant.

## Environment variables

| Variable | Default | Description |
| --- | --- | --- |
| `DSH_URL` | `http://127.0.0.1:3080` | Backend address |
| `DSH_NO_AUTOSTART` | off | Only probe; show error instead of starting dsh |
| `DSH_BACKEND_TIMEOUT` | `30` | Seconds to wait for the backend after spawning |
| `DSH_BACKEND_LOG` | `~/Library/Logs/DSH-backend.log` | Log file of the spawned backend |
| `DSH_DEBUG` | off | Enable shell log |
| `DSH_LOG` | `~/Library/Logs/DSH.log` | Shell log file (with `DSH_DEBUG=1`) |
| `DSH_DEVTOOLS` | off | Open WebKit developer tools (page-side diagnostics) |

## Debug mode

```bash
DSH_DEBUG=1 ./target/DSH.app/Contents/MacOS/DSH
# optionally: DSH_LOG=/tmp/dsh.log DSH_BACKEND_LOG=/tmp/dsh-backend.log
```

`DSH.log` records the bootstrap state machine (probe / runner / spawn / ready / timeout);
`DSH-backend.log` holds the spawned dsh's own output — the first place to look when
auto-start fails. Panics are also written to the log.

## Cross-platform

macOS is the primary target. The same code builds on Windows (WebView2) and
Linux (WebKitGTK) — the runner discovery is OS-aware (PATH separators,
`.exe/.cmd/.bat` on Windows, per-OS fallback dirs). CI builds all three platforms;
see `make_app.sh` for the macOS-only packaging step.

## CI & releases

CI is gated on the changelog: **only when `changelog/` gains a new version file
(`vX.Y.Z.md`) does CI build all three platforms and auto-publish a GitHub Release**
(tag = version, notes = changelog content, assets = macOS `.app` zip / Windows exe / Linux binary).
Manual runs: GitHub → Actions → Run workflow.

Flow: write code → create `changelog/vX.Y.Z.md` → push → CI builds → release appears.

## Known issues (fixed)

- **Crash on window deactivation** (early versions): wry's default `build()` replaces the
  window's contentView, which vanilla winit misinterprets on focus loss → segfault.
  Fixed by using **tao** (Tauri's winit fork) + wry's default `build()` — the same combo
  Tauri uses: no crash, and keyboard/IME input works normally.
- **Typing / IME lag**: caused by the child-webview workaround; resolved by the same
  tao + default-build switch.

## Repository

- `src/main.rs` — app entry, boot page, event loop
- `src/backend.rs` — bootstrap state machine (probe / runner / spawn / poll / fast-fail)
- `make_app.sh` — build + icon + `.app` assembly
- `assets/` — whale icon (source jpg, `DSH.icns`, boot-page base64 logo)
- `changelog/` — version entries that trigger CI/release
- `DSH-docs/` (outside this repo) — requirements & design docs

## License

Apache-2.0. The whale icon is DeepSeek's brand asset (used here as the app icon only).
