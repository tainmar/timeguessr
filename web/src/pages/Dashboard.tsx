import { useEffect, useState, type FormEvent } from "react";
import { Link, useNavigate } from "react-router";
import { api, ApiError, type GameListItem, type Organizer } from "../api";
import { useAuth } from "../auth";
import { OrganizerLayout } from "../components/Layout";
import { ErrorBox, GameStatusChip, ProgressBar, Spinner } from "../components/ui";
import { formatDateTime, plural } from "../lib/format";

export default function Dashboard() {
  const [games, setGames] = useState<GameListItem[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  const navigate = useNavigate();
  const { organizer, setOrganizer } = useAuth();
  const [myName, setMyName] = useState("");

  const saveName = async (e: FormEvent) => {
    e.preventDefault();
    try {
      setOrganizer(await api<Organizer>("/me", { method: "PATCH", body: { name: myName } }));
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "Could not save your name.");
    }
  };

  const load = () =>
    api<GameListItem[]>("/games")
      .then(setGames)
      .catch((e) => setError(e instanceof ApiError ? e.message : "Could not load your games."));

  useEffect(() => {
    load();
  }, []);

  const create = async (e: FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const res = await api<{ id: string }>("/games", { body: { name } });
      navigate(`/games/${res.id}`);
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "Could not create the game.");
      setBusy(false);
    }
  };

  return (
    <OrganizerLayout>
      <div className="page-head">
        <h1>My games</h1>
      </div>
      {organizer && !organizer.name && (
        <form className="card notice" onSubmit={saveName}>
          <label htmlFor="my-org-name">What's your name? Participants will see it in their invitation.</label>
          <div className="row">
            <input id="my-org-name" maxLength={80} value={myName} onChange={(e) => setMyName(e.target.value)} placeholder="e.g. Martin from HR" />
            <button className="btn" disabled={!myName.trim()}>
              Save
            </button>
          </div>
        </form>
      )}
      <form className="card create-game" onSubmit={create}>
        <label htmlFor="game-name">New game</label>
        <div className="row">
          <input
            id="game-name"
            required
            maxLength={100}
            placeholder="e.g. Marketing offsite 2026"
            value={name}
            onChange={(e) => setName(e.target.value)}
          />
          <button className="btn primary" disabled={busy || !name.trim()}>
            Create game
          </button>
        </div>
      </form>
      <ErrorBox error={error} onRetry={load} />
      {!games && !error && <Spinner />}
      {games && games.length === 0 && <p className="muted center pad">No games yet. Create your first one above!</p>}
      <div className="game-grid">
        {games?.map((g) => (
          <Link key={g.id} to={`/games/${g.id}`} className="card game-card" data-testid="game-card">
            <div className="game-card-head">
              <h3>{g.name}</h3>
              <GameStatusChip status={g.status} />
            </div>
            <ProgressBar
              value={g.ready_participants}
              max={g.participants}
              label={`${g.ready_participants}/${g.participants} participants ready · ${plural(g.photos, "photo")}`}
            />
            <p className="muted small">
              {g.status === "collecting" && g.deadline_at
                ? `Deadline ${formatDateTime(g.deadline_at, g.timezone)}`
                : g.teams > 0
                  ? plural(g.teams, "team")
                  : `Created ${formatDateTime(g.created_at)}`}
            </p>
          </Link>
        ))}
      </div>
    </OrganizerLayout>
  );
}
