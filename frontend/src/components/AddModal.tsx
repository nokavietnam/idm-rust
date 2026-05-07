import { useState, useRef, useEffect } from "react";

interface Props {
  onSubmit: (url: string, connections: number) => void;
  onClose: () => void;
}

export default function AddModal({ onSubmit, onClose }: Props) {
  const [url, setUrl] = useState("");
  const [connections, setConnections] = useState(8);
  const inputRef = useRef<HTMLInputElement>(null);

  // Auto-focus the URL input when the modal opens.
  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  // Close on Escape key.
  useEffect(() => {
    const handleKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", handleKey);
    return () => window.removeEventListener("keydown", handleKey);
  }, [onClose]);

  const handleSubmit = () => {
    const trimmed = url.trim();
    if (!trimmed) return;
    onSubmit(trimmed, connections);
  };

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h2>New Download</h2>

        <div className="modal-row">
          <div className="modal-field">
            <label className="modal-label" htmlFor="modal-url">
              URL
            </label>
            <input
              id="modal-url"
              ref={inputRef}
              className="modal-input"
              type="url"
              placeholder="https://example.com/file.zip"
              value={url}
              onChange={(e) => setUrl(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && handleSubmit()}
              spellCheck={false}
              autoComplete="off"
            />
          </div>

          <div className="modal-field">
            <label className="modal-label" htmlFor="modal-conn">
              Connections
            </label>
            <input
              id="modal-conn"
              className="modal-input modal-input-sm"
              type="number"
              min={1}
              max={32}
              value={connections}
              onChange={(e) =>
                setConnections(Math.max(1, parseInt(e.target.value, 10) || 1))
              }
            />
          </div>
        </div>

        <div className="modal-actions">
          <button
            className="btn-modal btn-modal-cancel"
            onClick={onClose}
          >
            Cancel
          </button>
          <button
            className="btn-modal btn-modal-primary"
            onClick={handleSubmit}
          >
            Download
          </button>
        </div>
      </div>
    </div>
  );
}
