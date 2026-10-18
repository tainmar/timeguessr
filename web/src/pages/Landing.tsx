import { useState, type FormEvent } from "react";
import { Link } from "react-router";
import { api, ApiError } from "../api";
import { useAuth } from "../auth";
import { Logo } from "../components/Layout";

export default function Landing() {
  const { organizer } = useAuth();
  const [email, setEmail] = useState("");
  const [sent, setSent] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      await api("/auth/request", { body: { email } });
      setSent(true);
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "Something went wrong.");
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="landing">
      <header className="topbar transparent">
        <Logo />
        {organizer && (
          <Link to="/dashboard" className="btn small">
            My games →
          </Link>
        )}
      </header>
      <section className="hero">
        <div className="hero-text">
          <h1>
            When and where
            <br />
            was <span className="accent">that</span> photo taken?
          </h1>
          <p className="lead">
            A TimeGuessr game made of your team's own photos. Collect a few pictures from each participant, then let
            teams guess the year and the place. Perfect for team building, offsites and reunions.
          </p>
          <div className="signin-card" id="start">
            {organizer ? (
              <>
                <p>
                  Signed in as <strong>{organizer.email}</strong>.
                </p>
                <Link to="/dashboard" className="btn primary block">
                  Go to my games
                </Link>
              </>
            ) : sent ? (
              <div data-testid="link-sent">
                <h3>Check your inbox ✉️</h3>
                <p>
                  We sent a sign-in link to <strong>{email}</strong>. It expires in 30 minutes.
                </p>
                <button className="link-btn" onClick={() => setSent(false)}>
                  Use another email
                </button>
              </div>
            ) : (
              <form onSubmit={submit}>
                <label htmlFor="email">Create a game: sign in with your email</label>
                <div className="row">
                  <input
                    id="email"
                    type="email"
                    required
                    autoComplete="email"
                    placeholder="you@company.com"
                    value={email}
                    onChange={(e) => setEmail(e.target.value)}
                  />
                  <button className="btn primary" disabled={busy}>
                    {busy ? "Sending…" : "Send me a link"}
                  </button>
                </div>
                {error && <p className="form-error">{error}</p>}
                <p className="hint">No password: we email you a magic link. Participants never need an account.</p>
              </form>
            )}
          </div>
        </div>
        <div className="hero-art" aria-hidden>
          <div className="polaroid p1">
            <div className="ph" />
            <span>1987?</span>
          </div>
          <div className="polaroid p2">
            <div className="ph" />
            <span>Lisbon?</span>
          </div>
          <div className="polaroid p3">
            <div className="ph" />
            <span>2004 · Tokyo</span>
          </div>
        </div>
      </section>
      <section className="steps">
        <div className="step">
          <span className="step-n">1</span>
          <h3>Invite participants</h3>
          <p>Enter their emails. Each one gets a personal upload link: no account, no app.</p>
        </div>
        <div className="step">
          <span className="step-n">2</span>
          <h3>Collect photos</h3>
          <p>Everyone uploads photos of themselves with the year and the exact spot on the map. Automatic reminders do the chasing.</p>
        </div>
        <div className="step">
          <span className="step-n">3</span>
          <h3>Play in teams</h3>
          <p>Share the link or QR code. Teams guess when and where: up to 1,000 points per photo, live leaderboard.</p>
        </div>
      </section>
      <footer className="footer">Made for teams who like a good guess. Maps © OpenStreetMap contributors.</footer>
    </div>
  );
}
