import { useCallback, useEffect, useRef, useState } from "react";
import { useNavigate } from "react-router";
import { api, type NotificationItem } from "../api";
import { timeAgo } from "../lib/format";

const POLL_MS = 20_000;

export default function NotificationBell() {
  const [items, setItems] = useState<NotificationItem[]>([]);
  const [unread, setUnread] = useState(0);
  const [open, setOpen] = useState(false);
  const navigate = useNavigate();
  const panel = useRef<HTMLDivElement>(null);

  const load = useCallback(async () => {
    try {
      const res = await api<{ items: NotificationItem[]; unread: number }>("/notifications");
      setItems(res.items);
      setUnread(res.unread);
    } catch {
      /* offline: keep the last state */
    }
  }, []);

  useEffect(() => {
    load();
    const t = setInterval(() => document.visibilityState === "visible" && load(), POLL_MS);
    window.addEventListener("tg:refresh-notifications", load);
    return () => {
      clearInterval(t);
      window.removeEventListener("tg:refresh-notifications", load);
    };
  }, [load]);

  useEffect(() => {
    if (!open) return;
    const onClick = (e: MouseEvent) => {
      if (panel.current && !panel.current.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", onClick);
    return () => document.removeEventListener("mousedown", onClick);
  }, [open]);

  const markAll = async () => {
    await api("/notifications/read", { body: { all: true } });
    load();
  };

  const openItem = async (n: NotificationItem) => {
    setOpen(false);
    if (!n.read_at) {
      await api("/notifications/read", { body: { ids: [n.id] } }).catch(() => {});
      load();
    }
    if (n.link) navigate(n.link);
  };

  return (
    <div className="bell" ref={panel}>
      <button
        className="icon-btn bell-btn"
        onClick={() => setOpen((o) => !o)}
        aria-label={`Notifications (${unread} unread)`}
        data-testid="bell"
      >
        <svg viewBox="0 0 24 24" width="22" height="22" aria-hidden>
          <path
            fill="currentColor"
            d="M12 22a2.5 2.5 0 0 0 2.45-2h-4.9A2.5 2.5 0 0 0 12 22Zm7-6V11a7 7 0 0 0-5.5-6.84V3a1.5 1.5 0 0 0-3 0v1.16A7 7 0 0 0 5 11v5l-2 2v1h18v-1l-2-2Z"
          />
        </svg>
        {unread > 0 && (
          <span className="badge" data-testid="bell-count">
            {unread > 99 ? "99+" : unread}
          </span>
        )}
      </button>
      {open && (
        <div className="bell-panel" data-testid="bell-panel">
          <div className="bell-head">
            <strong>Notifications</strong>
            {unread > 0 && (
              <button className="link-btn" onClick={markAll}>
                Mark all as read
              </button>
            )}
          </div>
          {items.length === 0 && <p className="muted pad">Nothing yet. You'll be notified when participants upload photos.</p>}
          <ul>
            {items.map((n) => (
              <li key={n.id} className={n.read_at ? "" : "unread"}>
                <button onClick={() => openItem(n)}>
                  <span className="n-title">{n.title}</span>
                  <span className="n-body">{n.body}</span>
                  <span className="n-time">{timeAgo(n.created_at)}</span>
                </button>
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}
