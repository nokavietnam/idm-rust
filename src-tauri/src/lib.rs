use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::{mpsc, Mutex};
use tokio_util::sync::CancellationToken;

use idm_rust::downloader::{self, DownloadState};

// ---------------------------------------------------------------------------
// Shared application state
// ---------------------------------------------------------------------------

pub struct AppState {
    downloads: Mutex<HashMap<String, DownloadHandle>>,
    next_id: AtomicU32,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            downloads: Mutex::new(HashMap::new()),
            next_id: AtomicU32::new(1),
        }
    }
}

struct DownloadHandle {
    cancel: CancellationToken,
    url: String,
    connections: usize,
    filename: String,
}

// ---------------------------------------------------------------------------
// Event payloads (sent to frontend via app.emit)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgressEvent {
    pub download_id: String,
    pub filename: String,
    pub percentage: f64,
    pub speed_mbps: f64,
    pub eta_seconds: f64,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub status: String, // "downloading" | "complete" | "paused" | "error"
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn invoke_download(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    url: String,
    connections: usize,
) -> Result<String, String> {
    let connections = connections.max(1);
    let id = format!("dl-{}", state.next_id.fetch_add(1, Ordering::SeqCst));
    let filename = downloader::url_to_filename(&url).map_err(|e| e.to_string())?;
    let cancel = CancellationToken::new();

    {
        let mut downloads = state.downloads.lock().await;
        downloads.insert(
            id.clone(),
            DownloadHandle {
                cancel: cancel.clone(),
                url: url.clone(),
                connections,
                filename: filename.clone(),
            },
        );
    }

    spawn_download(app, id.clone(), url, connections, filename, cancel);
    Ok(id)
}

#[tauri::command]
pub async fn pause_download(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    download_id: String,
) -> Result<(), String> {
    let downloads = state.downloads.lock().await;
    let handle = downloads.get(&download_id).ok_or("Download not found")?;
    handle.cancel.cancel();

    let _ = app.emit(
        "download-progress",
        &DownloadProgressEvent {
            download_id,
            filename: handle.filename.clone(),
            percentage: 0.0,
            speed_mbps: 0.0,
            eta_seconds: 0.0,
            downloaded_bytes: 0,
            total_bytes: 0,
            status: "paused".into(),
        },
    );
    Ok(())
}

#[tauri::command]
pub async fn resume_download(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    download_id: String,
) -> Result<(), String> {
    let (url, connections, filename) = {
        let downloads = state.downloads.lock().await;
        let h = downloads.get(&download_id).ok_or("Download not found")?;
        (h.url.clone(), h.connections, h.filename.clone())
    };

    let cancel = CancellationToken::new();
    {
        let mut downloads = state.downloads.lock().await;
        if let Some(h) = downloads.get_mut(&download_id) {
            h.cancel = cancel.clone();
        }
    }

    spawn_download(app, download_id, url, connections, filename, cancel);
    Ok(())
}

// ---------------------------------------------------------------------------
// Download + aggregator spawner (shared by invoke & resume)
// ---------------------------------------------------------------------------

fn spawn_download(
    app: AppHandle,
    download_id: String,
    url: String,
    connections: usize,
    filename: String,
    cancel: CancellationToken,
) {
    let (tx, mut rx) = mpsc::channel::<u64>(512);

    let already_downloaded: u64 = DownloadState::load(&filename)
        .map(|s| s.total_downloaded())
        .unwrap_or(0);

    // --- Aggregator: collects byte deltas → emits every 500ms -------------
    let app_agg = app.clone();
    let id_agg = download_id.clone();
    let fname_agg = filename.clone();

    tokio::spawn(async move {
        let mut downloaded: u64 = already_downloaded;
        let mut total_bytes: u64 = DownloadState::load(&fname_agg)
            .map(|s| s.total_size)
            .unwrap_or(0);
        let start = Instant::now();
        let mut last_emit = Instant::now();

        while let Some(delta) = rx.recv().await {
            downloaded += delta;

            if total_bytes == 0 {
                if let Some(s) = DownloadState::load(&fname_agg) {
                    total_bytes = s.total_size;
                }
            }

            let done = total_bytes > 0 && downloaded >= total_bytes;
            if last_emit.elapsed() >= Duration::from_millis(500) || done {
                let elapsed = start.elapsed().as_secs_f64();
                let new_bytes = downloaded.saturating_sub(already_downloaded);
                let speed = if elapsed > 0.0 {
                    new_bytes as f64 / elapsed / 1_048_576.0
                } else {
                    0.0
                };
                let pct = if total_bytes > 0 {
                    (downloaded as f64 / total_bytes as f64 * 100.0).min(100.0)
                } else {
                    0.0
                };
                let eta = if speed > 0.0 {
                    total_bytes.saturating_sub(downloaded) as f64 / (speed * 1_048_576.0)
                } else {
                    0.0
                };

                let _ = app_agg.emit(
                    "download-progress",
                    &DownloadProgressEvent {
                        download_id: id_agg.clone(),
                        filename: fname_agg.clone(),
                        percentage: pct,
                        speed_mbps: speed,
                        eta_seconds: eta,
                        downloaded_bytes: downloaded,
                        total_bytes,
                        status: if done {
                            "complete".into()
                        } else {
                            "downloading".into()
                        },
                    },
                );
                last_emit = Instant::now();
            }
        }

        // Final emit when channel closes.
        if total_bytes > 0 {
            let elapsed = start.elapsed().as_secs_f64();
            let new_bytes = downloaded.saturating_sub(already_downloaded);
            let speed = if elapsed > 0.0 {
                new_bytes as f64 / elapsed / 1_048_576.0
            } else {
                0.0
            };
            let _ = app_agg.emit(
                "download-progress",
                &DownloadProgressEvent {
                    download_id: id_agg.clone(),
                    filename: fname_agg.clone(),
                    percentage: (downloaded as f64 / total_bytes as f64 * 100.0).min(100.0),
                    speed_mbps: speed,
                    eta_seconds: 0.0,
                    downloaded_bytes: downloaded,
                    total_bytes,
                    status: if downloaded >= total_bytes {
                        "complete".into()
                    } else {
                        "paused".into()
                    },
                },
            );
        }
    });

    // --- Download task ----------------------------------------------------
    let app_dl = app.clone();
    let id_dl = download_id.clone();
    let fname_dl = filename.clone();

    tokio::spawn(async move {
        match downloader::download(&url, connections, cancel, Some(tx)).await {
            Ok(()) => {} // aggregator sends the "complete" event
            Err(downloader::DownloadError::Cancelled) => {
                let _ = app_dl.emit(
                    "download-progress",
                    &DownloadProgressEvent {
                        download_id: id_dl,
                        filename: fname_dl,
                        percentage: 0.0,
                        speed_mbps: 0.0,
                        eta_seconds: 0.0,
                        downloaded_bytes: 0,
                        total_bytes: 0,
                        status: "paused".into(),
                    },
                );
            }
            Err(e) => {
                let _ = app_dl.emit(
                    "download-progress",
                    &DownloadProgressEvent {
                        download_id: id_dl,
                        filename: fname_dl,
                        percentage: 0.0,
                        speed_mbps: 0.0,
                        eta_seconds: 0.0,
                        downloaded_bytes: 0,
                        total_bytes: 0,
                        status: format!("error: {e}"),
                    },
                );
            }
        }
    });
}

// ---------------------------------------------------------------------------
// Tauri app builder
// ---------------------------------------------------------------------------

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            invoke_download,
            pause_download,
            resume_download,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
