import type { DownloadItem } from "../types";
import DownloadRow from "./DownloadRow";

interface Props {
  downloads: DownloadItem[];
  onPause: (id: string) => void;
  onResume: (id: string) => void;
}

export default function DownloadList({ downloads, onPause, onResume }: Props) {
  if (downloads.length === 0) {
    return (
      <div className="empty-state">
        <svg
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.5"
          strokeLinecap="round"
          strokeLinejoin="round"
        >
          <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
          <polyline points="7 10 12 15 17 10" />
          <line x1="12" y1="15" x2="12" y2="3" />
        </svg>
        <p>No downloads yet</p>
        <span>Click the + button to add a URL</span>
      </div>
    );
  }

  return (
    <div className="download-list">
      {downloads.map((dl) => (
        <DownloadRow
          key={dl.id}
          item={dl}
          onPause={() => onPause(dl.id)}
          onResume={() => onResume(dl.id)}
        />
      ))}
    </div>
  );
}
