import { useState, useEffect, useCallback } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import type { DownloadItem, DownloadProgressPayload } from "./types";
import DownloadList from "./components/DownloadList";
import AddModal from "./components/AddModal";

export default function App() {
  const [downloads, setDownloads] = useState<Map<string, DownloadItem>>(
    new Map()
  );
  const [modalOpen, setModalOpen] = useState(false);

  // ---- Listen for progress events from Rust ----
  useEffect(() => {
    const unlisten = listen<DownloadProgressPayload>(
      "download-progress",
      (event) => {
        const p = event.payload;
        setDownloads((prev) => {
          const next = new Map(prev);
          const existing = next.get(p.downloadId);

          // Determine status
          let status: DownloadItem["status"];
          if (p.status === "complete") status = "complete";
          else if (p.status === "paused") status = "paused";
          else if (p.status.startsWith("error")) status = "error";
          else status = "downloading";

          next.set(p.downloadId, {
            id: p.downloadId,
            filename: p.filename || existing?.filename || "unknown",
            url: existing?.url || "",
            connections: existing?.connections || 8,
            percentage: p.percentage || existing?.percentage || 0,
            speedMbps: p.speedMbps,
            etaSeconds: p.etaSeconds,
            downloadedBytes: p.downloadedBytes || existing?.downloadedBytes || 0,
            totalBytes: p.totalBytes || existing?.totalBytes || 0,
            status,
            errorMessage: p.status.startsWith("error:")
              ? p.status.slice(7)
              : undefined,
          });
          return next;
        });
      }
    );

    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  // ---- Actions ----
  const handleAdd = useCallback(
    async (url: string, connections: number) => {
      try {
        const id = await invoke<string>("invoke_download", {
          url,
          connections,
        });

        // Extract filename from URL for immediate display
        let filename = "download";
        try {
          const parts = new URL(url).pathname.split("/").filter(Boolean);
          if (parts.length) filename = decodeURIComponent(parts[parts.length - 1]);
        } catch {}

        setDownloads((prev) => {
          const next = new Map(prev);
          next.set(id, {
            id,
            filename,
            url,
            connections,
            percentage: 0,
            speedMbps: 0,
            etaSeconds: 0,
            downloadedBytes: 0,
            totalBytes: 0,
            status: "downloading",
          });
          return next;
        });

        setModalOpen(false);
      } catch (err) {
        console.error("invoke_download failed:", err);
      }
    },
    []
  );

  const handlePause = useCallback(async (id: string) => {
    try {
      await invoke("pause_download", { downloadId: id });
    } catch (err) {
      console.error("pause_download failed:", err);
    }
  }, []);

  const handleResume = useCallback(async (id: string) => {
    try {
      await invoke("resume_download", { downloadId: id });
      setDownloads((prev) => {
        const next = new Map(prev);
        const item = next.get(id);
        if (item) next.set(id, { ...item, status: "downloading" });
        return next;
      });
    } catch (err) {
      console.error("resume_download failed:", err);
    }
  }, []);

  const downloadList = Array.from(downloads.values());

  return (
    <div className="app-shell">
      {/* Toolbar */}
      <header className="toolbar">
        <div className="toolbar-left">
          <svg
            className="toolbar-logo"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            strokeLinecap="round"
            strokeLinejoin="round"
          >
            <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
            <polyline points="7 10 12 15 17 10" />
            <line x1="12" y1="15" x2="12" y2="3" />
          </svg>
          <h1 className="toolbar-title">
            IDM <span>Rust</span>
          </h1>
        </div>

        <button className="btn-add" onClick={() => setModalOpen(true)}>
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
            <line x1="12" y1="5" x2="12" y2="19" />
            <line x1="5" y1="12" x2="19" y2="12" />
          </svg>
          New Download
        </button>
      </header>

      {/* Download list */}
      <DownloadList
        downloads={downloadList}
        onPause={handlePause}
        onResume={handleResume}
      />

      {/* Modal */}
      {modalOpen && (
        <AddModal
          onSubmit={handleAdd}
          onClose={() => setModalOpen(false)}
        />
      )}
    </div>
  );
}
