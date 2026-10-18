import type { ReactNode } from "react";
import { Link, useNavigate } from "react-router";
import { api } from "../api";
import { useAuth } from "../auth";
import NotificationBell from "./NotificationBell";

export function Logo() {
  return (
    <Link to="/" className="logo">
      <img src="/favicon.svg" alt="" width={28} height={28} />
      <span>
        Time<b>Guessr</b>
      </span>
    </Link>
  );
}

/** Organizer pages: header with notifications and account menu. */
export function OrganizerLayout({ children }: { children: ReactNode }) {
  const { organizer, setOrganizer } = useAuth();
  const navigate = useNavigate();
  const logout = async () => {
    await api("/auth/logout", { method: "POST", body: {} }).catch(() => {});
    setOrganizer(null);
    navigate("/");
  };
  return (
    <div className="app">
      <header className="topbar">
        <Logo />
        <nav className="topbar-actions">
          {organizer && (
            <>
              <Link to="/dashboard" className="nav-link">
                My games
              </Link>
              <NotificationBell />
              <Link to="/settings" className="nav-link account" title={organizer.email}>
                {organizer.name || organizer.email}
              </Link>
              <button className="link-btn" onClick={logout}>
                Sign out
              </button>
            </>
          )}
        </nav>
      </header>
      <main className="container">{children}</main>
    </div>
  );
}

/** Participant & player pages: minimal chrome. */
export function PublicLayout({ children, wide }: { children: ReactNode; wide?: boolean }) {
  return (
    <div className="app">
      <header className="topbar slim">
        <Logo />
      </header>
      <main className={wide ? "container wide" : "container"}>{children}</main>
    </div>
  );
}
