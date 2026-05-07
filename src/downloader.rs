//! # Multi-threaded File Downloader with Pause/Resume
//!
//! Downloads a file by splitting it into byte-range segments and fetching them
//! concurrently via [`tokio::spawn`]. Each segment is written directly to the
//! correct offset in the output file using [`tokio::fs::File`] and
//! [`AsyncSeekExt::seek`].
//!
//! Download progress is tracked via [`DownloadState`] and persisted to a
//! `.json` sidecar file. If a download is interrupted (e.g. Ctrl+C), calling
//! [`download`] again with the same URL will automatically resume from where
//! each segment left off.
//!
//! ## Example
//!
//! ```no_run
//! use idm_rust::downloader::download;
//! use tokio_util::sync::CancellationToken;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let cancel = CancellationToken::new();
//!     download("https://example.com/large-file.bin", 8, cancel).await?;
//!     Ok(())
//! }
//! ```

use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures::StreamExt;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::fs::File;
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tokio::sync::mpsc;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

// ---------------------------------------------------------------------------
// Progress channel
// ---------------------------------------------------------------------------

/// Sender half of the progress channel.
///
/// Each value is the number of new bytes written in a single chunk.
/// The receiver can aggregate these deltas to compute percentage, speed, etc.
pub type ProgressTx = mpsc::Sender<u64>;

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

/// Errors that can occur during a multi-segment download.
#[derive(Debug, Error)]
pub enum DownloadError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Server does not advertise Accept-Ranges: bytes")]
    RangeNotSupported,

    #[error("Unable to determine Content-Length from HEAD response")]
    UnknownContentLength,

    #[error("Invalid URL — could not extract a filename")]
    NoFilename,

    #[error("Segment {segment} failed: {source}")]
    SegmentFailed {
        segment: usize,
        source: Box<DownloadError>,
    },

    #[error("Download was cancelled — state saved for resume")]
    Cancelled,

    #[error("Failed to serialize/deserialize state: {0}")]
    StateSerialize(String),
}

/// Convenient result alias.
pub type Result<T> = std::result::Result<T, DownloadError>;

// ---------------------------------------------------------------------------
// Download state (pause / resume)
// ---------------------------------------------------------------------------

/// Tracks the progress of every segment so a download can be resumed.
///
/// Serialised to `<filename>.idm.json` beside the output file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadState {
    /// The original URL.
    pub url: String,
    /// Total file size in bytes.
    pub total_size: u64,
    /// Output filename (basename only).
    pub filename: String,
    /// Per-segment progress.
    pub segments: Vec<SegmentState>,
}

/// Progress for a single byte-range segment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentState {
    /// First byte of this segment (inclusive).
    pub start: u64,
    /// Last byte of this segment (inclusive).
    pub end: u64,
    /// Number of bytes successfully written so far.
    pub bytes_written: u64,
}

impl SegmentState {
    /// The byte offset where downloading should resume.
    fn resume_offset(&self) -> u64 {
        self.start + self.bytes_written
    }

    /// Remaining bytes to download for this segment.
    fn remaining(&self) -> u64 {
        (self.end - self.start + 1).saturating_sub(self.bytes_written)
    }

    /// Whether this segment has been fully downloaded.
    fn is_complete(&self) -> bool {
        self.bytes_written >= self.end - self.start + 1
    }
}

impl DownloadState {
    /// Build a fresh state for a new download.
    pub fn new(url: &str, filename: &str, total_size: u64, connections: usize) -> Self {
        let ranges = split_ranges(total_size, connections);
        let segments = ranges
            .iter()
            .map(|r| SegmentState {
                start: r.start,
                end: r.end,
                bytes_written: 0,
            })
            .collect();

        Self {
            url: url.to_owned(),
            total_size,
            filename: filename.to_owned(),
            segments,
        }
    }

    /// Path to the sidecar JSON state file.
    fn state_path(filename: &str) -> PathBuf {
        PathBuf::from(format!("{}.idm.json", filename))
    }

    /// Try to load an existing state file from disk.
    pub fn load(filename: &str) -> Option<Self> {
        let path = Self::state_path(filename);
        let data = std::fs::read_to_string(&path).ok()?;
        serde_json::from_str(&data).ok()
    }

    /// Persist the current state to disk.
    pub fn save(&self) -> Result<()> {
        let path = Self::state_path(&self.filename);
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| DownloadError::StateSerialize(e.to_string()))?;
        std::fs::write(&path, json)?;
        Ok(())
    }

    /// Remove the state file (called after a successful download).
    pub fn cleanup(&self) {
        let path = Self::state_path(&self.filename);
        let _ = std::fs::remove_file(path);
    }

    /// Whether every segment is complete.
    fn is_complete(&self) -> bool {
        self.segments.iter().all(|s| s.is_complete())
    }

    /// Total bytes already downloaded across all segments.
    pub fn total_downloaded(&self) -> u64 {
        self.segments.iter().map(|s| s.bytes_written).sum()
    }
}

// ---------------------------------------------------------------------------
// Byte-range helper
// ---------------------------------------------------------------------------

/// An inclusive byte range `[start, end]` for a single download segment.
#[derive(Debug, Clone, Copy)]
struct ByteRange {
    start: u64,
    end: u64,
}

/// Split `total_bytes` into `n` roughly equal ranges.
fn split_ranges(total_bytes: u64, n: usize) -> Vec<ByteRange> {
    let chunk = total_bytes / n as u64;
    let remainder = total_bytes % n as u64;

    let mut ranges = Vec::with_capacity(n);
    let mut offset: u64 = 0;

    for i in 0..n {
        // Distribute the remainder across the first `remainder` segments.
        let extra = if (i as u64) < remainder { 1 } else { 0 };
        let size = chunk + extra;
        let end = offset + size - 1;

        ranges.push(ByteRange { start: offset, end });
        offset = end + 1;
    }

    ranges
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Download a file from `url` using `connections` concurrent segments.
///
/// The function:
/// 1. Sends an HTTP **HEAD** request to discover `Content-Length` and
///    verify that the server supports `Accept-Ranges: bytes`.
/// 2. Checks for an existing `.idm.json` state file to resume a
///    previous download. If none exists, creates fresh state.
/// 3. Pre-allocates the output file to the full size (or opens the
///    existing partial file for resuming).
/// 4. Spawns one [`tokio::spawn`] task per incomplete segment, each
///    streaming its segment and writing to the shared file at the
///    correct offset via [`AsyncSeekExt::seek`].
/// 5. Periodically saves state. If `cancel` is triggered (e.g. via
///    Ctrl+C), all segments stop gracefully and state is persisted.
///
/// The output file is saved in the current directory with the filename
/// extracted from the URL path.
///
/// # Cancellation
///
/// Pass a [`CancellationToken`] to allow graceful shutdown. When the
/// token is cancelled, every segment finishes writing its current
/// chunk, state is saved, and the function returns
/// [`DownloadError::Cancelled`].
///
/// # Errors
///
/// Returns [`DownloadError`] if the server does not support range requests,
/// the content length is unknown, or any segment encounters an HTTP/IO error.
pub async fn download(
    url: &str,
    connections: usize,
    cancel: CancellationToken,
    progress_tx: Option<ProgressTx>,
) -> Result<()> {
    let connections = connections.max(1);
    let client = Client::new();

    // ----- 1. HEAD request ------------------------------------------------
    let head = client.head(url).send().await?.error_for_status()?;

    // Check Accept-Ranges
    let accepts_ranges = head
        .headers()
        .get(reqwest::header::ACCEPT_RANGES)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.eq_ignore_ascii_case("bytes"))
        .unwrap_or(false);

    if !accepts_ranges {
        return Err(DownloadError::RangeNotSupported);
    }

    // Content-Length
    let total_size = head
        .content_length()
        .ok_or(DownloadError::UnknownContentLength)?;

    // Derive filename from URL
    let filename = url_to_filename(url)?;

    // ----- 2. Load or create state ----------------------------------------
    let (state, resuming) = match DownloadState::load(&filename) {
        Some(existing)
            if existing.url == url
                && existing.total_size == total_size
                && !existing.is_complete() =>
        {
            let downloaded = existing.total_downloaded();
            println!(
                "Resuming \"{}\" — {}/{} bytes already downloaded ({} segments)",
                filename,
                downloaded,
                total_size,
                existing.segments.len()
            );
            (existing, true)
        }
        _ => {
            println!(
                "Downloading \"{}\" ({} bytes) with {} connection(s)…",
                filename, total_size, connections
            );
            (
                DownloadState::new(url, &filename, total_size, connections),
                false,
            )
        }
    };

    let state = Arc::new(Mutex::new(state));

    // ----- 3. Open / pre-allocate file ------------------------------------
    let path = Path::new(&filename);
    let file = if resuming {
        // Open existing file for writing without truncating.
        tokio::fs::OpenOptions::new()
            .write(true)
            .open(path)
            .await?
    } else {
        let f = File::create(path).await?;
        f.set_len(total_size).await?;
        f
    };

    let file = Arc::new(Mutex::new(file));

    // Snapshot segments so we can iterate without holding the lock.
    let segments: Vec<(usize, SegmentState)> = {
        let st = state.lock().await;
        st.segments.iter().cloned().enumerate().collect()
    };

    // ----- 4. Progress bars -----------------------------------------------
    let multi = MultiProgress::new();
    let style = ProgressStyle::with_template(
        "  segment {msg:>2} [{bar:30.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec})",
    )
    .unwrap()
    .progress_chars("█▓▒░  ");

    // ----- 5. Spawn concurrent segment tasks ------------------------------
    let mut handles = Vec::new();

    for (i, seg) in &segments {
        // Skip already-complete segments.
        if seg.is_complete() {
            continue;
        }

        let client = client.clone();
        let url = url.to_owned();
        let file = Arc::clone(&file);
        let state = Arc::clone(&state);
        let cancel = cancel.clone();
        let seg = seg.clone();
        let idx = *i;

        let remaining = seg.remaining();
        let pb = multi.add(ProgressBar::new(seg.end - seg.start + 1));
        pb.set_style(style.clone());
        pb.set_message(format!("{}", idx + 1));
        // Advance the progress bar to reflect already-downloaded bytes.
        pb.set_position(seg.bytes_written);

        let ptx = progress_tx.clone();
        let handle = tokio::spawn(async move {
            download_segment(&client, &url, &file, &state, &seg, idx, &pb, &cancel, ptx)
                .await
        });

        handles.push((idx, handle));
    }

    // ----- 6. Await all tasks ---------------------------------------------
    let mut cancelled = false;

    for (i, handle) in handles {
        match handle.await {
            Ok(Ok(())) => {}
            Ok(Err(DownloadError::Cancelled)) => {
                cancelled = true;
                // Don't break — let other tasks finish their current chunk.
            }
            Ok(Err(e)) => {
                // Save state before propagating the error.
                let st = state.lock().await;
                let _ = st.save();
                return Err(DownloadError::SegmentFailed {
                    segment: i,
                    source: Box::new(e),
                });
            }
            Err(join_err) => {
                let st = state.lock().await;
                let _ = st.save();
                return Err(DownloadError::SegmentFailed {
                    segment: i,
                    source: Box::new(DownloadError::Io(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        join_err,
                    ))),
                });
            }
        }
    }

    // ----- 7. Final state handling ----------------------------------------
    let st = state.lock().await;

    if cancelled {
        st.save()?;
        println!("⏸  Download paused — state saved to {}.idm.json", filename);
        return Err(DownloadError::Cancelled);
    }

    if st.is_complete() {
        st.cleanup();
        println!("✓ Download complete → {}", filename);
    } else {
        // Shouldn't happen, but save just in case.
        st.save()?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

/// Download a single byte-range segment, writing directly to `file` at the
/// correct offset. Updates `state` after every chunk so progress is resumable.
/// If `progress_tx` is provided, sends byte-count deltas after each write.
async fn download_segment(
    client: &Client,
    url: &str,
    file: &Arc<Mutex<File>>,
    state: &Arc<Mutex<DownloadState>>,
    seg: &SegmentState,
    index: usize,
    pb: &ProgressBar,
    cancel: &CancellationToken,
    progress_tx: Option<ProgressTx>,
) -> Result<()> {
    let resume_from = seg.resume_offset();
    let range_header = format!("bytes={}-{}", resume_from, seg.end);

    let response = client
        .get(url)
        .header(reqwest::header::RANGE, &range_header)
        .send()
        .await?
        .error_for_status()?;

    let mut stream = response.bytes_stream();
    let mut offset = resume_from;

    loop {
        tokio::select! {
            biased;

            // Check cancellation first.
            _ = cancel.cancelled() => {
                // Save progress before returning.
                let st = state.lock().await;
                let _ = st.save();
                pb.abandon_with_message(format!("{} ⏸", index + 1));
                return Err(DownloadError::Cancelled);
            }

            chunk_opt = stream.next() => {
                match chunk_opt {
                    Some(Ok(chunk)) => {
                        // Write to file at the correct offset.
                        {
                            let mut f = file.lock().await;
                            f.seek(std::io::SeekFrom::Start(offset)).await?;
                            f.write_all(&chunk).await?;
                        }

                        let len = chunk.len() as u64;
                        offset += len;

                        // Update segment state.
                        {
                            let mut st = state.lock().await;
                            st.segments[index].bytes_written += len;
                        }

                        // Notify progress channel.
                        if let Some(ref tx) = progress_tx {
                            let _ = tx.send(len).await;
                        }

                        pb.inc(len);
                    }
                    Some(Err(e)) => {
                        // Save progress before propagating.
                        let st = state.lock().await;
                        let _ = st.save();
                        pb.abandon_with_message(format!("{} ✗", index + 1));
                        return Err(DownloadError::Http(e));
                    }
                    None => {
                        // Stream finished — segment complete.
                        break;
                    }
                }
            }
        }
    }

    pb.finish_with_message(format!("{} ✓", index + 1));
    Ok(())
}

/// Extract a usable filename from a URL, falling back to the last path
/// segment.
pub fn url_to_filename(url: &str) -> Result<String> {
    let parsed = reqwest::Url::parse(url).map_err(|_| DownloadError::NoFilename)?;

    let name = parsed
        .path_segments()
        .and_then(|segs| segs.last())
        .filter(|s| !s.is_empty())
        .ok_or(DownloadError::NoFilename)?;

    Ok(name.to_string())
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_ranges_even() {
        let ranges = split_ranges(100, 4);
        assert_eq!(ranges.len(), 4);
        assert_eq!(ranges[0].start, 0);
        assert_eq!(ranges[0].end, 24);
        assert_eq!(ranges[1].start, 25);
        assert_eq!(ranges[1].end, 49);
        assert_eq!(ranges[2].start, 50);
        assert_eq!(ranges[2].end, 74);
        assert_eq!(ranges[3].start, 75);
        assert_eq!(ranges[3].end, 99);
    }

    #[test]
    fn test_split_ranges_uneven() {
        // 10 bytes, 3 segments → 4 + 3 + 3
        let ranges = split_ranges(10, 3);
        assert_eq!(ranges.len(), 3);
        assert_eq!(ranges[0].start, 0);
        assert_eq!(ranges[0].end, 3); // 4 bytes
        assert_eq!(ranges[1].start, 4);
        assert_eq!(ranges[1].end, 6); // 3 bytes
        assert_eq!(ranges[2].start, 7);
        assert_eq!(ranges[2].end, 9); // 3 bytes
    }

    #[test]
    fn test_split_ranges_single() {
        let ranges = split_ranges(50, 1);
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].start, 0);
        assert_eq!(ranges[0].end, 49);
    }

    #[test]
    fn test_url_to_filename() {
        let name = url_to_filename("https://example.com/files/archive.zip").unwrap();
        assert_eq!(name, "archive.zip");
    }

    #[test]
    fn test_url_to_filename_no_path() {
        let result = url_to_filename("https://example.com/");
        assert!(result.is_err());
    }

    // --- DownloadState tests ---

    #[test]
    fn test_state_new() {
        let state = DownloadState::new("https://example.com/file.bin", "file.bin", 100, 4);
        assert_eq!(state.segments.len(), 4);
        assert_eq!(state.total_size, 100);
        assert!(!state.is_complete());
        assert_eq!(state.total_downloaded(), 0);
    }

    #[test]
    fn test_segment_resume_offset() {
        let seg = SegmentState {
            start: 100,
            end: 199,
            bytes_written: 50,
        };
        assert_eq!(seg.resume_offset(), 150);
        assert_eq!(seg.remaining(), 50);
        assert!(!seg.is_complete());
    }

    #[test]
    fn test_segment_complete() {
        let seg = SegmentState {
            start: 0,
            end: 99,
            bytes_written: 100,
        };
        assert!(seg.is_complete());
        assert_eq!(seg.remaining(), 0);
    }

    #[test]
    fn test_state_complete() {
        let mut state = DownloadState::new("https://example.com/f.bin", "f.bin", 10, 2);
        // Manually mark all segments as complete.
        for seg in &mut state.segments {
            seg.bytes_written = seg.end - seg.start + 1;
        }
        assert!(state.is_complete());
        assert_eq!(state.total_downloaded(), 10);
    }

    #[test]
    fn test_state_save_and_load() {
        let state = DownloadState::new(
            "https://example.com/test_save.bin",
            "test_save.bin",
            256,
            2,
        );
        state.save().unwrap();

        let loaded = DownloadState::load("test_save.bin").unwrap();
        assert_eq!(loaded.url, state.url);
        assert_eq!(loaded.total_size, 256);
        assert_eq!(loaded.segments.len(), 2);

        // Cleanup test file.
        state.cleanup();
    }

    #[test]
    fn test_state_path() {
        let path = DownloadState::state_path("archive.zip");
        assert_eq!(path, PathBuf::from("archive.zip.idm.json"));
    }
}
