import { useCallback, useEffect, useMemo, useState, type FormEvent } from "react";
import { Link, useParams } from "react-router";
import { api, ApiError, loadTeamToken, saveTeamToken, type PlayInfo, type PlayState, type Reveal, type TeamRef } from "../api";
import { Logo } from "../components/Layout";
import MapView, { type MapMarker } from "../components/MapView";
import { ErrorBox, Spinner } from "../components/ui";
import YearSlider from "../components/YearSlider";
import ZoomableImage from "../components/ZoomableImage";
import { formatDistance, greatCircle, wrapLng, type LngLat } from "../lib/geo";

function errorMessage(e: unknown) {
  return e instanceof ApiError ? e.message : "Something went wrong.";
}

export default function Play() {
  const { code = "" } = useParams();
  const [info, setInfo] = useState<PlayInfo | null>(null);
  const [token, setToken] = useState<string | null>(() => loadTeamToken(code));
  const [state, setState] = useState<PlayState | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [newTeam, setNewTeam] = useState<TeamRef | null>(null);

  useEffect(() => {
    api<PlayInfo>(`/play/${code}`)
      .then(setInfo)
      .catch((e) => setError(e instanceof ApiError && e.status === 404 ? "This game link isn't valid (or the game isn't open yet)." : errorMessage(e)));
  }, [code]);

  const loadState = useCallback(async () => {
    if (!token) return;
    try {
      setState(await api<PlayState>(`/play/${code}/state`, { teamToken: token }));
      setError(null);
    } catch (e) {
      if (e instanceof ApiError && e.status === 403) {
        saveTeamToken(code, null);
        setToken(null);
        setState(null);
      } else {
        setError(errorMessage(e));
      }
    }
  }, [code, token]);

  useEffect(() => {
    loadState();
  }, [loadState]);

  const onJoined = (t: string, team: TeamRef) => {
    saveTeamToken(code, t);
    setToken(t);
    setNewTeam(team);
  };

  const leave = () => {
    if (!confirm(`Leave this device's team session? You can rejoin with the team name and PIN ${state?.team.pin ?? ""}.`)) return;
    saveTeamToken(code, null);
    setToken(null);
    setState(null);
  };

  return (
    <div className="app play-app">
      <header className="topbar slim">
        <Logo />
        {state && (
          <div className="play-header-info">
            <span className="team-badge" data-testid="team-badge" title={`Team PIN: ${state.team.pin}`}>
              👥 {state.team.name} · PIN {state.team.pin}
            </span>
            <span className="score-badge" data-testid="total-score">
              {state.total_score.toLocaleString("en-US")} pts
            </span>
            <button className="link-btn small" onClick={leave}>
              Switch team
            </button>
          </div>
        )}
      </header>
      <main className="play-main">
        {error && <ErrorBox error={error} onRetry={token ? loadState : undefined} />}
        {!info && !error && <Spinner />}
        {info && !token && <JoinForm code={code} info={info} onJoined={onJoined} />}
        {info && token && !state && !error && <Spinner />}
        {state && (
          <Game
            code={code}
            token={token!}
            state={state}
            reload={loadState}
            onScore={(total) => setState((s) => (s ? { ...s, total_score: total } : s))}
            newTeam={newTeam}
            dismissNewTeam={() => setNewTeam(null)}
          />
        )}
      </main>
    </div>
  );
}

function JoinForm({ code, info, onJoined }: { code: string; info: PlayInfo; onJoined: (token: string, team: TeamRef) => void }) {
  const [name, setName] = useState("");
  const [pin, setPin] = useState("");
  const [needPin, setNeedPin] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const res = await api<{ token: string; team: TeamRef }>(`/play/${code}/teams`, {
        body: { name, pin: needPin ? pin : undefined },
      });
      onJoined(res.token, res.team);
    } catch (err) {
      if (err instanceof ApiError && err.status === 409 && !needPin && /PIN/.test(err.message)) setNeedPin(true);
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="join">
      <div className="card join-card">
        <p className="eyebrow">{info.organizer} invites you to play</p>
        <h1 data-testid="play-title">{info.name}</h1>
        <p className="lead">
          {info.photo_count} photos. For each one, guess the <strong>year</strong> and the <strong>place</strong>: up to 500 + 500
          points per photo.
        </p>
        {info.status === "finished" ? (
          <>
            <p>This game has ended.</p>
            <Link className="btn primary" to={`/play/${code}/leaderboard`}>
              See the leaderboard
            </Link>
          </>
        ) : (
          <form onSubmit={submit}>
            <label htmlFor="team-name">Team name</label>
            <input
              id="team-name"
              required
              minLength={2}
              maxLength={40}
              value={name}
              onChange={(e) => {
                setName(e.target.value);
                setNeedPin(false);
              }}
              placeholder="e.g. The Time Travellers"
              autoFocus
              data-testid="team-name"
            />
            {needPin && (
              <>
                <label htmlFor="team-pin">Team PIN (shown on your teammate's screen)</label>
                <input
                  id="team-pin"
                  inputMode="numeric"
                  pattern="[0-9]{4}"
                  maxLength={4}
                  value={pin}
                  onChange={(e) => setPin(e.target.value)}
                  placeholder="1234"
                  data-testid="team-pin"
                />
              </>
            )}
            {error && <p className="form-error">{error}</p>}
            <button className="btn primary block big" disabled={busy || (!info.joining_open && !needPin)} data-testid="join">
              {needPin ? "Join my team" : "Start playing"}
            </button>
            {!info.joining_open && <p className="hint">The organizer has closed joining for new teams. Existing teams can rejoin with their PIN.</p>}
          </form>
        )}
      </div>
      <Link className="link-btn" to={`/play/${code}/leaderboard`}>
        Leaderboard →
      </Link>
    </div>
  );
}

function Game({
  code,
  token,
  state,
  reload,
  onScore,
  newTeam,
  dismissNewTeam,
}: {
  code: string;
  token: string;
  state: PlayState;
  reload: () => Promise<void>;
  onScore: (total: number) => void;
  newTeam: TeamRef | null;
  dismissNewTeam: () => void;
}) {
  const [reveal, setReveal] = useState<Reveal | null>(null);

  // Preload the next photo while the team is thinking.
  useEffect(() => {
    if (state.next_image_url) new Image().src = state.next_image_url;
  }, [state.next_image_url]);

  if (reveal) {
    return (
      <RevealView
        reveal={reveal}
        code={code}
        onNext={async () => {
          await reload();
          setReveal(null);
          window.scrollTo({ top: 0 });
        }}
      />
    );
  }

  if (state.finished || !state.round) {
    return (
      <div className="card narrow center finished" data-testid="finished">
        <h1>🏁 Finished!</h1>
        <p className="big-score">{state.total_score.toLocaleString("en-US")} points</p>
        <p>
          out of {(state.total_rounds * 1000).toLocaleString("en-US")} possible, over {state.total_rounds} photos.
        </p>
        <Link className="btn primary big" to={`/play/${code}/leaderboard`} data-testid="see-leaderboard">
          See the leaderboard
        </Link>
      </div>
    );
  }

  if (state.game.status === "finished") {
    return (
      <div className="card narrow center">
        <h2>The organizer has ended the game</h2>
        <Link className="btn primary" to={`/play/${code}/leaderboard`}>
          See the leaderboard
        </Link>
      </div>
    );
  }

  return (
    <>
      {newTeam && (
        <div className="pin-banner" data-testid="pin-banner">
          Team <strong>{newTeam.name}</strong> created. Teammates can join from their own phone with the same name and PIN{" "}
          <strong className="pin">{newTeam.pin}</strong>.
          <button className="icon-btn" onClick={dismissNewTeam} aria-label="Dismiss">
            ✕
          </button>
        </div>
      )}
      <Round
        key={state.round.photo_id}
        code={code}
        token={token}
        state={state}
        onRevealed={(r) => {
          setReveal(r);
          onScore(r.total_score);
        }}
      />
    </>
  );
}

function Round({
  code,
  token,
  state,
  onRevealed,
}: {
  code: string;
  token: string;
  state: PlayState;
  onRevealed: (r: Reveal) => void;
}) {
  const round = state.round!;
  const { year_min, year_max } = state.game;
  const [year, setYear] = useState(Math.round((year_min + year_max) / 2));
  const [pick, setPick] = useState<{ lat: number; lng: number } | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const markers = useMemo<MapMarker[]>(
    () => (pick ? [{ id: "guess", lat: pick.lat, lng: pick.lng, color: "#2563eb", title: "Your guess" }] : []),
    [pick],
  );

  const submit = async () => {
    if (!pick) return;
    setBusy(true);
    setError(null);
    try {
      const r = await api<Reveal>(`/play/${code}/guess`, {
        teamToken: token,
        body: { photo_id: round.photo_id, year, lat: pick.lat, lng: wrapLng(pick.lng) },
      });
      onRevealed(r);
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="round" data-testid="round">
      <div className="round-photo">
        <div className="round-counter" data-testid="round-counter">
          Photo {round.number} / {round.total}
        </div>
        <ZoomableImage src={round.image_url} alt={`Photo ${round.number}`} testId="round-photo" />
      </div>
      <div className="round-panel">
        <section className="panel-block">
          <h3>When?</h3>
          <YearSlider min={year_min} max={year_max} value={year} onChange={setYear} />
        </section>
        <section className="panel-block grow">
          <h3>Where? {pick ? <span className="muted small">pin placed: tap to move it</span> : <span className="muted small">tap the map</span>}</h3>
          <MapView className="guess-map" testId="guess-map" markers={markers} onPick={(lng, lat) => setPick({ lng, lat })} />
        </section>
        {error && (
          <p className="form-error">
            {error}{" "}
            <button className="link-btn" onClick={submit}>
              Retry
            </button>
          </p>
        )}
        <button className="btn primary big block sticky-submit" disabled={!pick || busy} onClick={submit} data-testid="submit-guess">
          {busy ? "Checking…" : pick ? `Submit ${year} & this place` : "Place your pin on the map"}
        </button>
      </div>
    </div>
  );
}

function ScoreBar({ label, points, detail }: { label: string; points: number; detail: string }) {
  return (
    <div className="score-row">
      <div className="score-row-head">
        <span>{label}</span>
        <strong>{points} / 500</strong>
      </div>
      <div className="score-track">
        <div className="score-fill" style={{ width: `${(points / 500) * 100}%` }} />
      </div>
      <div className="muted small">{detail}</div>
    </div>
  );
}

function RevealView({ reveal, code, onNext }: { reveal: Reveal; code: string; onNext: () => void }) {
  const [loading, setLoading] = useState(false);
  const { guess, answer } = reveal;
  const line = useMemo(() => greatCircle([guess.lng, guess.lat], [answer.lng, answer.lat]), [guess, answer]);
  const markers = useMemo<MapMarker[]>(() => {
    const end = line[line.length - 1];
    return [
      { id: "guess", lng: line[0][0], lat: line[0][1], color: "#2563eb", label: "You", title: "Your guess" },
      { id: "answer", lng: end[0], lat: end[1], color: "#e8a33d", label: "✓", title: "Actual place" },
    ];
  }, [line]);
  const fit = useMemo<LngLat[]>(() => line, [line]);
  const yearsOff = Math.abs(guess.year - answer.year);

  return (
    <div className="reveal" data-testid="reveal">
      <div className="reveal-photo">
        <ZoomableImage src={reveal.image_url} alt="Photo" />
        <div className="reveal-caption">
          <span>
            📸 <strong>{answer.owner}</strong>
          </span>
          {answer.caption && <span> · “{answer.caption}”</span>}
        </div>
      </div>
      <div className="reveal-panel">
        <div className="reveal-score">
          <span className="round-points" data-testid="round-score">
            +{reveal.score}
          </span>
          <span className="muted">
            Total {reveal.total_score.toLocaleString("en-US")} · photo {reveal.round_number}/{reveal.total_rounds}
          </span>
        </div>
        <ScoreBar
          label={`Year: ${answer.year}`}
          points={reveal.year_score}
          detail={yearsOff === 0 ? "Spot on! 🎯" : `You said ${guess.year}: ${yearsOff} year${yearsOff > 1 ? "s" : ""} off`}
        />
        <ScoreBar
          label="Place"
          points={reveal.location_score}
          detail={reveal.distance_km < 0.05 ? "Right there! 🎯" : `${formatDistance(reveal.distance_km)} away`}
        />
        <MapView className="reveal-map" testId="reveal-map" markers={markers} lines={[line]} fitTo={fit} />
        {reveal.finished ? (
          <Link className="btn primary big block" to={`/play/${code}/leaderboard`} onClick={() => setLoading(true)} data-testid="next">
            See the final leaderboard →
          </Link>
        ) : (
          <button
            className="btn primary big block"
            disabled={loading}
            onClick={async () => {
              setLoading(true);
              await onNext();
            }}
            data-testid="next"
          >
            Next photo →
          </button>
        )}
      </div>
    </div>
  );
}
