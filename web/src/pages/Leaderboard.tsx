import { useCallback, useEffect, useMemo, useState } from "react";
import { Link, useParams } from "react-router";
import { api, ApiError, loadTeamToken, type Leaderboard as LeaderboardData, type LeaderboardPhoto } from "../api";
import { Logo } from "../components/Layout";
import Lightbox from "../components/Lightbox";
import MapView, { type MapMarker } from "../components/MapView";
import { ErrorBox, Spinner } from "../components/ui";
import { formatDistance, type LngLat } from "../lib/geo";

const REFRESH_MS = 10_000;
const TEAM_COLORS = ["#2563eb", "#db2777", "#16a34a", "#9333ea", "#ea580c", "#0891b2", "#4f46e5", "#be123c", "#65a30d", "#0d9488"];

export default function Leaderboard() {
  const { code = "" } = useParams();
  const [data, setData] = useState<LeaderboardData | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [updatedAt, setUpdatedAt] = useState<number>(0);
  const [now, setNow] = useState(Date.now());
  const [openPhoto, setOpenPhoto] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      const d = await api<LeaderboardData>(`/play/${code}/leaderboard`, { teamToken: loadTeamToken(code) });
      setData(d);
      setUpdatedAt(Date.now());
      setError(null);
    } catch (e) {
      setError(e instanceof ApiError ? e.message : "Could not load the leaderboard.");
    }
  }, [code]);

  useEffect(() => {
    load();
    const refresh = setInterval(load, REFRESH_MS);
    const tick = setInterval(() => setNow(Date.now()), 1000);
    return () => {
      clearInterval(refresh);
      clearInterval(tick);
    };
  }, [load]);

  const fullscreen = () => {
    if (document.fullscreenElement) document.exitFullscreen();
    else document.documentElement.requestFullscreen?.().catch(() => {});
  };

  const photoIndex = data && openPhoto ? data.photos.findIndex((p) => p.id === openPhoto) : -1;
  const current = data && photoIndex >= 0 ? data.photos[photoIndex] : null;
  const seconds = updatedAt ? Math.max(0, Math.round((now - updatedAt) / 1000)) : 0;

  return (
    <div className="app leaderboard-app">
      <header className="topbar slim">
        <Logo />
        <div className="row">
          <span className="muted small refresh-indicator" data-testid="refresh-indicator">
            <span className="live-dot" /> Live · updated {seconds}s ago
          </span>
          <button className="btn small ghost" onClick={fullscreen}>
            ⛶ Fullscreen
          </button>
        </div>
      </header>
      <main className="container wide">
        {error && <ErrorBox error={error} onRetry={load} />}
        {!data && !error && <Spinner />}
        {data && (
          <>
            <div className="page-head">
              <h1>🏆 {data.game.name}</h1>
              {data.game.status === "finished" ? (
                <span className="chip dark">Game over: answers revealed</span>
              ) : (
                <Link to={`/play/${code}`} className="btn small">
                  {data.viewer_team_id ? "Back to the game" : "Join the game"}
                </Link>
              )}
            </div>
            {!data.reveal_answers && (
              <p className="hint">
                Click a photo to zoom in. Answers appear here once your team has finished (or when the organizer ends the game).
              </p>
            )}
            {data.teams.length === 0 ? (
              <p className="muted center pad">No team has joined yet.</p>
            ) : (
              <div className="table-wrap leaderboard-wrap">
                <table className="table leaderboard" data-testid="leaderboard">
                  <thead>
                    <tr>
                      <th className="rank">#</th>
                      <th className="team-col">Team</th>
                      {data.photos.map((p) => (
                        <th key={p.id} className="photo-col">
                          <button className="thumb-btn" onClick={() => setOpenPhoto(p.id)} aria-label={`Open photo ${p.number}`} data-testid="leaderboard-thumb">
                            <img src={p.thumb_url} alt="" loading="lazy" />
                            <span>{p.answer ? p.answer.year : `#${p.number}`}</span>
                          </button>
                        </th>
                      ))}
                      <th className="total-col">Total</th>
                    </tr>
                  </thead>
                  <tbody>
                    {data.teams.map((t, i) => (
                      <tr key={t.id} className={t.id === data.viewer_team_id ? "mine" : ""} data-testid="leaderboard-row">
                        <td className="rank">{["🥇", "🥈", "🥉"][i] ?? i + 1}</td>
                        <td className="team-col">
                          <div className="strong">{t.name}</div>
                          {!t.finished && (
                            <div className="muted small">
                              playing… {t.rounds_played}/{t.rounds_total}
                            </div>
                          )}
                        </td>
                        {data.photos.map((p) => {
                          const s = t.scores[p.id];
                          return (
                            <td
                              key={p.id}
                              className="score-cell"
                              title={s ? `Year ${s.year_score} + place ${s.location_score}` : "Not played yet"}
                              style={s ? { background: `rgba(232, 163, 61, ${(s.score / 1000) * 0.55})` } : undefined}
                            >
                              {s ? s.score : "·"}
                            </td>
                          );
                        })}
                        <td className="total-col strong" data-testid="team-total">
                          {t.total.toLocaleString("en-US")}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            )}
            <p className="muted small center">Refreshes automatically every 10 seconds.</p>
          </>
        )}
      </main>
      {data && current && (
        <Lightbox
          src={current.image_url}
          alt={`Photo ${current.number}`}
          onClose={() => setOpenPhoto(null)}
          onPrev={photoIndex > 0 ? () => setOpenPhoto(data.photos[photoIndex - 1].id) : undefined}
          onNext={photoIndex < data.photos.length - 1 ? () => setOpenPhoto(data.photos[photoIndex + 1].id) : undefined}
        >
          <PhotoDetails photo={current} />
        </Lightbox>
      )}
    </div>
  );
}

function PhotoDetails({ photo }: { photo: LeaderboardPhoto }) {
  const answer = photo.answer;
  const guesses = photo.guesses ?? [];
  const markers = useMemo<MapMarker[]>(() => {
    if (!answer) return [];
    return [
      ...guesses.map((g, i) => ({
        id: g.team_id,
        lat: g.lat,
        lng: g.lng,
        color: TEAM_COLORS[i % TEAM_COLORS.length],
        label: g.team_name,
        title: `${g.team_name}: ${g.year}, ${formatDistance(g.distance_km)} away`,
      })),
      { id: "answer", lat: answer.lat, lng: answer.lng, color: "#e8a33d", label: "✓", title: "Actual place" },
    ];
    // Keep marker identity stable across 10 s refreshes unless the data changed.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [JSON.stringify(answer), JSON.stringify(guesses)]);
  const fit = useMemo<LngLat[] | null>(
    () => (answer ? [[answer.lng, answer.lat], ...guesses.map((g) => [g.lng, g.lat] as LngLat)] : null),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [JSON.stringify(answer), guesses.length],
  );

  if (!answer) {
    return (
      <div>
        <h3>Photo {photo.number}</h3>
        <p className="muted">Answers are revealed once your team has finished.</p>
      </div>
    );
  }
  return (
    <div data-testid="photo-details">
      <h3>
        {answer.year} · {answer.owner}
      </h3>
      {answer.caption && <p className="muted">“{answer.caption}”</p>}
      <MapView className="side-map" markers={markers} fitTo={fit} />
      {guesses.length > 0 && (
        <ul className="guess-list">
          {[...guesses]
            .sort((a, b) => b.score - a.score)
            .map((g) => (
              <li key={g.team_id}>
                <strong>{g.team_name}</strong>: {g.year}, {formatDistance(g.distance_km)} away → <strong>{g.score}</strong>
              </li>
            ))}
        </ul>
      )}
    </div>
  );
}
