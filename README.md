<p align="center">
  <img src="https://img.shields.io/badge/Rust-000000?style=for-the-badge&logo=rust&logoColor=white" alt="Rust" />
  <img src="https://img.shields.io/badge/Tauri_v2-24C8DB?style=for-the-badge&logo=tauri&logoColor=white" alt="Tauri v2" />
  <img src="https://img.shields.io/badge/Tokio-463B2E?style=for-the-badge" alt="Tokio" />
  <img src="https://img.shields.io/badge/License-Apache_2.0-blue?style=for-the-badge" alt="License" />
</p>

# ⚡ IDM Rust

**A multi-threaded download accelerator** built with Rust, powered by [Tokio](https://tokio.rs/) and [Reqwest](https://docs.rs/reqwest), with a sleek desktop GUI via [Tauri v2](https://v2.tauri.app/).

IDM Rust splits files into byte-range segments and downloads them concurrently — just like Internet Download Manager — with full **pause/resume** support through persistent JSON state files.

---

## ✨ Features

- **Multi-threaded downloads** — splits files into *N* segments fetched in parallel via HTTP `Range` requests
- **Pause & resume** — download state is persisted to a `.idm.json` sidecar file; interrupted downloads resume automatically
- **Cancellation-safe** — graceful shutdown via `CancellationToken`; each segment flushes its buffer before stopping
- **Real-time progress** — `mpsc` channel streams byte deltas to the UI at ~500 ms intervals with speed, ETA, and percentage
- **Tauri v2 desktop app** — modern dark-themed UI with glassmorphism, animated progress bar, and event log
- **Standalone library** — the core downloader (`idm-rust` crate) can be used independently in any Rust project

---

## 🏗️ Architecture

```
┌────────────────────────────────────────────────────────┐
│                     Tauri v2 Shell                     │
│  ┌──────────────┐   events    ┌─────────────────────┐  │
│  │   Frontend   │ ◄────────── │   src-tauri/lib.rs   │  │
│  │  (HTML/JS)   │ ──invoke──► │  Tauri Commands      │  │
│  └──────────────┘             └────────┬────────────┘  │
│                                        │               │
│                          ┌─────────────▼─────────────┐ │
│                          │   idm-rust (core crate)   │ │
│                          │  src/downloader.rs         │ │
│                          │                           │ │
│                          │  ┌─────┐ ┌─────┐ ┌─────┐ │ │
│                          │  │Seg 1│ │Seg 2│ │Seg N│ │ │
│                          │  └──┬──┘ └──┬──┘ └──┬──┘ │ │
│                          │     │       │       │     │ │
│                          │     ▼       ▼       ▼     │ │
│                          │     ┌───────────────┐     │ │
│                          │     │  Output File  │     │ │
│                          │     └───────────────┘     │ │
│                          └───────────────────────────┘ │
└────────────────────────────────────────────────────────┘
```

Each segment opens its **own file handle** with a `BufWriter` (256 KiB buffer), seeks to the correct offset, and writes sequentially — eliminating all file-level mutex contention.

---

## 📋 Prerequisites

| Tool | Version |
|------|---------|
| [Rust](https://rustup.rs/) | 1.70+ (2021 edition) |
| [Node.js](https://nodejs.org/) | 18+ |
| [Tauri v2 prerequisites](https://v2.tauri.app/start/prerequisites/) | WebView2 (Windows), webkit2gtk (Linux) |

---

## 🚀 Getting Started

### Clone the repository

```bash
git clone https://github.com/nokavietnam/idm-rust.git
cd idm-rust
```

### Install frontend dependencies

```bash
cd frontend
npm install
cd ..
```

### Run the desktop app (dev mode)

```bash
cd src-tauri
cargo tauri dev
```

This starts the Vite dev server on `http://localhost:5173` and launches the Tauri window.

### Build for production

```bash
cd src-tauri
cargo tauri build
```

---

## 📦 Using the Core Library

The `idm-rust` crate can be used standalone without Tauri:

```toml
# Cargo.toml
[dependencies]
idm-rust = { path = "." }
tokio = { version = "1", features = ["full"] }
tokio-util = "0.7"
```

```rust
use idm_rust::downloader::download;
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cancel = CancellationToken::new();
    download("https://example.com/large-file.bin", 8, cancel, None).await?;
    Ok(())
}
```

### With progress tracking

```rust
use tokio::sync::mpsc;

let (tx, mut rx) = mpsc::channel::<u64>(256);

// Spawn receiver to aggregate byte deltas
tokio::spawn(async move {
    let mut total = 0u64;
    while let Some(delta) = rx.recv().await {
        total += delta;
        println!("Downloaded: {} bytes", total);
    }
});

download(url, 8, cancel, Some(tx)).await?;
```

---

## 📂 Project Structure

```
idm-rust/
├── Cargo.toml              # Core library manifest
├── LICENSE                  # Apache 2.0
├── src/
│   ├── lib.rs              # Crate root — re-exports downloader module
│   └── downloader.rs       # Multi-threaded download engine
├── src-tauri/
│   ├── Cargo.toml          # Tauri app manifest (depends on idm-rust)
│   ├── tauri.conf.json     # Tauri window & build configuration
│   └── src/
│       ├── main.rs          # Tauri entry point
│       └── lib.rs           # Tauri commands, event aggregator, app state
├── frontend/               # Tauri frontend (Vite + React + TypeScript)
│   ├── package.json
│   ├── vite.config.ts
│   └── src/
└── ui/                     # Standalone UI prototype
    ├── index.html           # Dark-themed download manager UI
    ├── styles.css           # Design system (CSS custom properties)
    └── main.js              # Tauri IPC integration (invoke/listen)
```

---

## ⚙️ How It Works

1. **HEAD request** — discovers `Content-Length` and verifies `Accept-Ranges: bytes` support
2. **State check** — looks for an existing `.idm.json` file to resume a previous download
3. **File pre-allocation** — creates the output file at full size (`set_len`) for new downloads
4. **Concurrent segments** — spawns *N* `tokio::spawn` tasks, each streaming its byte range via HTTP `Range` headers
5. **Buffered I/O** — each segment writes through a 256 KiB `BufWriter` to minimize syscalls
6. **Progress channel** — byte deltas are sent over an `mpsc` channel; the Tauri aggregator batches them into UI events every ~500 ms
7. **Graceful shutdown** — `CancellationToken` triggers a coordinated stop; all segments flush and state is persisted

---

## 🎨 UI Design

The desktop UI features:

- **Dark mode** with electric blue accents and radial glow effects
- **Animated progress bar** with a shimmer overlay
- **Real-time stats** — downloaded / total size, speed (MB/s), and ETA
- **Status badges** — Downloading, Complete, Paused, Error
- **Event log** — timestamped monospace log with color-coded severity
- **Inter font** via Google Fonts for clean, modern typography

---

## 🧪 Running Tests

```bash
cargo test
```

The core crate includes unit tests for:
- Byte-range splitting (even, uneven, single segment)
- URL-to-filename extraction
- Download state creation, serialization, and resumption
- Segment progress tracking (resume offset, remaining bytes, completion)

---

## 🤝 Contributing

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/awesome-feature`)
3. Commit your changes (`git commit -m 'Add awesome feature'`)
4. Push to the branch (`git push origin feature/awesome-feature`)
5. Open a Pull Request

---

## 📄 License

This project is licensed under the **Apache License 2.0** — see the [LICENSE](LICENSE) file for details.
