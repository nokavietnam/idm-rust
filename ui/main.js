// ---------------------------------------------------------------------------
// IDM Rust — Frontend logic
//
// Uses the Tauri v2 global API (withGlobalTauri: true) to invoke commands
// and listen for events emitted by the Rust backend.
// ---------------------------------------------------------------------------

const { invoke } = window.__TAURI__.core;
const { listen }  = window.__TAURI__.event;

// ---- DOM refs ----
const urlInput         = document.getElementById("url-input");
const connectionsInput = document.getElementById("connections-input");
const downloadBtn      = document.getElementById("download-btn");
const progressSection  = document.getElementById("progress-section");
const progressFilename = document.getElementById("progress-filename");
const progressPercent  = document.getElementById("progress-percent");
const progressBarFill  = document.getElementById("progress-bar-fill");
const statDownloaded   = document.getElementById("stat-downloaded");
const statTotal        = document.getElementById("stat-total");
const statSpeed        = document.getElementById("stat-speed");
const statEta          = document.getElementById("stat-eta");
const statusBadge      = document.getElementById("status-badge");
const statusText       = document.getElementById("status-text");
const logSection       = document.getElementById("log-section");
const logEntries       = document.getElementById("log-entries");

// ---- Helpers ----

function formatBytes(bytes) {
  if (bytes === 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.floor(Math.log(bytes) / Math.log(1024));
  const value = bytes / Math.pow(1024, i);
  return `${value.toFixed(i > 0 ? 2 : 0)} ${units[i]}`;
}

function formatEta(seconds) {
  if (!seconds || seconds <= 0) return "—";
  if (seconds < 60) return `${Math.ceil(seconds)}s`;
  if (seconds < 3600) {
    const m = Math.floor(seconds / 60);
    const s = Math.ceil(seconds % 60);
    return `${m}m ${s}s`;
  }
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  return `${h}h ${m}m`;
}

function filenameFromUrl(url) {
  try {
    const pathname = new URL(url).pathname;
    const parts = pathname.split("/").filter(Boolean);
    return parts.length > 0 ? decodeURIComponent(parts[parts.length - 1]) : "download";
  } catch {
    return "download";
  }
}

function logMessage(msg, level = "info") {
  logSection.classList.remove("hidden");
  const time = new Date().toLocaleTimeString("en-GB", { hour12: false });
  const entry = document.createElement("div");
  entry.className = "log-entry";
  entry.innerHTML = `<span class="log-time">${time}</span><span class="log-msg-${level}">${msg}</span>`;
  logEntries.appendChild(entry);
  logEntries.scrollTop = logEntries.scrollHeight;
}

function setStatus(text, level) {
  statusText.textContent = text;
  statusBadge.className = `status-badge status-${level}`;
}

// ---- Event listeners ----

downloadBtn.addEventListener("click", async () => {
  const url = urlInput.value.trim();
  if (!url) {
    urlInput.focus();
    return;
  }

  const connections = parseInt(connectionsInput.value, 10) || 8;

  // Show progress section
  progressSection.classList.remove("hidden");
  progressFilename.textContent = filenameFromUrl(url);
  progressPercent.textContent = "0%";
  progressBarFill.style.width = "0%";
  statDownloaded.textContent = "0 MB";
  statTotal.textContent = "—";
  statSpeed.textContent = "—";
  statEta.textContent = "—";
  setStatus("Downloading…", "downloading");

  downloadBtn.disabled = true;

  logMessage(`Starting download: ${url} (${connections} connections)`, "info");

  try {
    await invoke("invoke_download", { url, connections });
    logMessage("Download task launched", "info");
  } catch (err) {
    logMessage(`Failed to start: ${err}`, "error");
    setStatus("Error", "error");
    downloadBtn.disabled = false;
  }
});

// Allow pressing Enter in the URL field.
urlInput.addEventListener("keydown", (e) => {
  if (e.key === "Enter") downloadBtn.click();
});

// ---- Tauri event: progress updates (every ~500ms) ----

listen("download-progress", (event) => {
  const d = event.payload;

  const pct = d.percentage.toFixed(1);
  progressPercent.textContent = `${pct}%`;
  progressBarFill.style.width = `${d.percentage}%`;

  statDownloaded.textContent = formatBytes(d.downloadedBytes);
  statTotal.textContent = formatBytes(d.totalBytes);
  statSpeed.textContent = `${d.speedMbps.toFixed(2)} MB/s`;
  statEta.textContent = formatEta(d.etaSeconds);

  if (d.isComplete) {
    setStatus("✓ Complete", "complete");
    downloadBtn.disabled = false;
    logMessage("Download complete!", "success");
    // Stop shimmer animation
    document.querySelector(".progress-bar-shimmer").style.animation = "none";
  }
});

// ---- Tauri event: download paused ----

listen("download-paused", (event) => {
  setStatus("⏸ Paused", "paused");
  downloadBtn.disabled = false;
  logMessage("Download paused — resume by clicking Download again", "warning");
});

// ---- Tauri event: download error ----

listen("download-error", (event) => {
  const msg = event.payload.message || "Unknown error";
  setStatus("Error", "error");
  downloadBtn.disabled = false;
  logMessage(`Error: ${msg}`, "error");
});
