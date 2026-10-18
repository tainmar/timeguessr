import { useEffect, useState, type ReactNode } from "react";
import type { GameStatus, ParticipantStatus } from "../api";

export function Spinner({ label = "Loading…" }: { label?: string }) {
  return (
    <div className="spinner-wrap" role="status">
      <span className="spinner" />
      <span>{label}</span>
    </div>
  );
}

export function ErrorBox({ error, onRetry }: { error: string | null; onRetry?: () => void }) {
  if (!error) return null;
  return (
    <div className="error-box" role="alert">
      <span>{error}</span>
      {onRetry && (
        <button className="btn small ghost" onClick={onRetry}>
          Retry
        </button>
      )}
    </div>
  );
}

export function CopyButton({ text, label = "Copy", testId }: { text: string; label?: string; testId?: string }) {
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    if (!copied) return;
    const t = setTimeout(() => setCopied(false), 1600);
    return () => clearTimeout(t);
  }, [copied]);
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      const ta = document.createElement("textarea");
      ta.value = text;
      document.body.appendChild(ta);
      ta.select();
      document.execCommand("copy");
      ta.remove();
    }
    setCopied(true);
  };
  return (
    <button type="button" className="btn small ghost" onClick={copy} data-testid={testId}>
      {copied ? "Copied ✓" : label}
    </button>
  );
}

const GAME_STATUS: Record<GameStatus, { label: string; tone: string }> = {
  draft: { label: "Draft", tone: "muted" },
  collecting: { label: "Collecting photos", tone: "info" },
  ready: { label: "Playing", tone: "success" },
  finished: { label: "Ended", tone: "dark" },
};

export function GameStatusChip({ status }: { status: GameStatus }) {
  const s = GAME_STATUS[status];
  return <span className={`chip ${s.tone}`}>{s.label}</span>;
}

const PARTICIPANT_STATUS: Record<ParticipantStatus, { label: string; tone: string }> = {
  not_started: { label: "Not started", tone: "muted" },
  partial: { label: "Partial", tone: "warn" },
  ready: { label: "Ready", tone: "success" },
};

export function ParticipantStatusChip({ status, done }: { status: ParticipantStatus; done?: boolean }) {
  const s = PARTICIPANT_STATUS[status];
  return (
    <span className={`chip ${s.tone}`} data-testid="participant-status">
      {s.label}
      {status === "ready" && done ? " (done)" : ""}
    </span>
  );
}

export function ProgressBar({ value, max, label }: { value: number; max: number; label?: ReactNode }) {
  const pct = max > 0 ? Math.round((value / max) * 100) : 0;
  return (
    <div className="progress">
      <div className="progress-track">
        <div className="progress-fill" style={{ width: `${pct}%` }} />
      </div>
      {label && <div className="progress-label">{label}</div>}
    </div>
  );
}

export function Modal({ title, onClose, children }: { title: string; onClose: () => void; children: ReactNode }) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);
  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="modal" role="dialog" aria-modal="true" aria-label={title}>
        <div className="modal-head">
          <h3>{title}</h3>
          <button className="icon-btn" onClick={onClose} aria-label="Close">
            ✕
          </button>
        </div>
        {children}
      </div>
    </div>
  );
}

let toastListener: ((msg: string, tone: "ok" | "error") => void) | null = null;

export function toast(msg: string, tone: "ok" | "error" = "ok") {
  toastListener?.(msg, tone);
}

export function Toaster() {
  const [items, setItems] = useState<{ id: number; msg: string; tone: string }[]>([]);
  useEffect(() => {
    toastListener = (msg, tone) => {
      const id = Date.now() + Math.random();
      setItems((xs) => [...xs, { id, msg, tone }]);
      setTimeout(() => setItems((xs) => xs.filter((x) => x.id !== id)), 4200);
    };
    return () => {
      toastListener = null;
    };
  }, []);
  return (
    <div className="toaster" aria-live="polite">
      {items.map((t) => (
        <div key={t.id} className={`toast ${t.tone}`}>
          {t.msg}
        </div>
      ))}
    </div>
  );
}
