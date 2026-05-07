import type { DownloadItem } from "../types";

interface Props {
  item: DownloadItem;
  onPause: () => void;
  onResume: () => void;
}

function formatBytes(bytes: number): string {
  if (bytes === 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.floor(Math.log(bytes) / Math.log(1024));
  const value = bytes / Math.pow(1024, i);
  return `${value.toFixed(i > 0 ? 2 : 0)} ${units[i]}`;
}

function formatEta(seconds: number): string {
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

/** Pause icon (two vertical bars) */
const PauseIcon = () => (
  <svg viewBox="0 0 24 24" fill="currentColor">
    <rect x="6" y="4" width="4" height="16" rx="1" />
    <rect x="14" y="4" width="4" height="16" rx="1" />
  </svg>
);

/** Play / resume icon (triangle) */
const PlayIcon = () => (
  <svg viewBox="0 0 24 24" fill="currentColor">
    <polygon points="6,4 20,12 6,20" />
  </svg>
);

export default function DownloadRow({ item, onPause, onResume }: Props) {
  const {
    filename,
    percentage,
    speedMbps,
    etaSeconds,
    downloadedBytes,
    totalBytes,
    status,
  } = item;

  const statusClass =
    status === "error" ? "error" : status === "complete" ? "complete" : status === "paused" ? "paused" : "downloading";

  const isActive = status === "downloading";
  const isPaused = status === "paused";

  return (
    <div className="dl-row">
      {/* File info */}
      <div className="dl-file">
        <div className="dl-filename" title={filename}>
          {filename}
        </div>
        <div className="dl-meta">
          <span>
            {formatBytes(downloadedBytes)} / {totalBytes > 0 ? formatBytes(totalBytes) : "—"}
          </span>
          <span className={`dl-status ${statusClass}`}>
            {status === "error" ? "Error" : status.charAt(0).toUpperCase() + status.slice(1)}
          </span>
        </div>
      </div>

      {/* Progress bar */}
      <div className="dl-progress">
        <div className="dl-progress-track">
          <div
            className={`dl-progress-fill ${statusClass}`}
            style={{ width: `${Math.min(percentage, 100)}%` }}
          />
        </div>
        <div className="dl-progress-label">{percentage.toFixed(1)}%</div>
      </div>

      {/* Speed */}
      <div className="dl-speed">
        {isActive ? `${speedMbps.toFixed(2)} MB/s` : "—"}
      </div>

      {/* ETA */}
      <div className="dl-eta">
        {isActive ? formatEta(etaSeconds) : "—"}
      </div>

      {/* Pause / Resume button */}
      <button
        className="btn-action"
        onClick={isActive ? onPause : isPaused ? onResume : undefined}
        title={isActive ? "Pause" : isPaused ? "Resume" : ""}
        disabled={status === "complete" || status === "error"}
        style={
          status === "complete" || status === "error"
            ? { opacity: 0.3, cursor: "default" }
            : undefined
        }
      >
        {isActive ? <PauseIcon /> : <PlayIcon />}
      </button>
    </div>
  );
}
