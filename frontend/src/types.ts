export interface DownloadItem {
  id: string;
  filename: string;
  url: string;
  connections: number;
  percentage: number;
  speedMbps: number;
  etaSeconds: number;
  downloadedBytes: number;
  totalBytes: number;
  status: "downloading" | "paused" | "complete" | "error" | "queued";
  errorMessage?: string;
}

/** Payload from Rust `app.emit("download-progress", ...)` */
export interface DownloadProgressPayload {
  downloadId: string;
  filename: string;
  percentage: number;
  speedMbps: number;
  etaSeconds: number;
  downloadedBytes: number;
  totalBytes: number;
  status: string;
}
