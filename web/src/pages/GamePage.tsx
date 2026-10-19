import { useCallback, useEffect, useMemo, useState, type FormEvent } from "react";
import { Link, useNavigate, useParams } from "react-router";
import { QRCodeSVG } from "qrcode.react";
import { api, ApiError, type GameDetail, type OrganizerPhoto, type ParticipantView } from "../api";
import { useAuth } from "../auth";
import { OrganizerLayout } from "../components/Layout";
import Lightbox from "../components/Lightbox";
import MapView from "../components/MapView";
import {
  CopyButton,
  ErrorBox,
  GameStatusChip,
  Modal,
  ParticipantStatusChip,
  ProgressBar,
  Spinner,
  toast,
} from "../components/ui";
import { browserTimeZone, formatDateTime, plural, timeAgo, toLocalInput } from "../lib/format";

const REMINDER_PRESETS = [7, 3, 1, 0];

function errorMessage(e: unknown) {
  return e instanceof ApiError ? e.message : "Something went wrong.";
}

export default function GamePage() {
  const { id } = useParams();
  const [detail, setDetail] = useState<GameDetail | null>(null);
  const [photos, setPhotos] = useState<OrganizerPhoto[]>([]);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      const [d, p] = await Promise.all([
        api<GameDetail>(`/games/${id}`),
        api<OrganizerPhoto[]>(`/games/${id}/photos`),
      ]);
      setDetail(d);
      setPhotos(p);
      setError(null);
    } catch (e) {
      setError(errorMessage(e));
    }
  }, [id]);

  useEffect(() => {
    load();
  }, [load]);

  // Live updates while photos come in or teams play.
  const status = detail?.game.status;
  useEffect(() => {
    if (status !== "collecting" && status !== "ready") return;
    const t = setInterval(() => document.visibilityState === "visible" && load(), 15_000);
    return () => clearInterval(t);
  }, [status, load]);

  const run = useCallback(
    async (fn: () => Promise<unknown>, success?: string) => {
      try {
        await fn();
        if (success) toast(success);
        await load();
        window.dispatchEvent(new Event("tg:refresh-notifications"));
      } catch (e) {
        toast(errorMessage(e), "error");
      }
    },
    [load],
  );

  if (!detail) {
    return (
      <OrganizerLayout>
        {error ? <ErrorBox error={error} onRetry={load} /> : <Spinner />}
      </OrganizerLayout>
    );
  }

  const { game } = detail;
  return (
    <OrganizerLayout>
      <Link to="/dashboard" className="back-link">
        ← My games
      </Link>
      <GameHeader detail={detail} run={run} />
      <Stepper status={game.status} />
      {game.status === "draft" && (
        <>
          <SettingsCard detail={detail} run={run} />
          <ParticipantsCard detail={detail} run={run} />
          <StartCollectingCard detail={detail} run={run} />
        </>
      )}
      {game.status === "collecting" && (
        <>
          <CollectionCard detail={detail} run={run} />
          <ParticipantsCard detail={detail} run={run} />
          <PhotosCard detail={detail} photos={photos} run={run} />
          <SettingsCard detail={detail} run={run} />
        </>
      )}
      {(game.status === "ready" || game.status === "finished") && (
        <>
          <PlayCard detail={detail} run={run} />
          <TeamsCard detail={detail} run={run} />
          <PhotosCard detail={detail} photos={photos} run={run} />
          <ParticipantsCard detail={detail} run={run} />
        </>
      )}
      <DangerZone detail={detail} />
    </OrganizerLayout>
  );
}

type Run = (fn: () => Promise<unknown>, success?: string) => Promise<void>;

function GameHeader({ detail, run }: { detail: GameDetail; run: Run }) {
  const { game } = detail;
  const [editing, setEditing] = useState(false);
  const [name, setName] = useState(game.name);
  const save = async (e: FormEvent) => {
    e.preventDefault();
    await run(() => api(`/games/${game.id}`, { method: "PATCH", body: { name } }), "Name updated");
    setEditing(false);
  };
  return (
    <div className="page-head game-head">
      {editing ? (
        <form onSubmit={save} className="row">
          <input value={name} onChange={(e) => setName(e.target.value)} maxLength={100} autoFocus />
          <button className="btn primary small">Save</button>
          <button type="button" className="btn ghost small" onClick={() => setEditing(false)}>
            Cancel
          </button>
        </form>
      ) : (
        <h1 data-testid="game-title">
          {game.name}{" "}
          <button className="icon-btn edit" onClick={() => setEditing(true)} aria-label="Rename game">
            ✎
          </button>
        </h1>
      )}
      <GameStatusChip status={game.status} />
    </div>
  );
}

function Stepper({ status }: { status: string }) {
  const steps = [
    ["draft", "Set up"],
    ["collecting", "Collect photos"],
    ["ready", "Play"],
    ["finished", "Reveal"],
  ];
  const current = steps.findIndex(([s]) => s === status);
  return (
    <ol className="stepper">
      {steps.map(([s, label], i) => (
        <li key={s} className={i < current ? "done" : i === current ? "current" : ""}>
          <span>{i + 1}</span>
          {label}
        </li>
      ))}
    </ol>
  );
}

function SettingsCard({ detail, run }: { detail: GameDetail; run: Run }) {
  const { game } = detail;
  const [photos, setPhotos] = useState(game.photos_per_participant);
  const [message, setMessage] = useState(game.message ?? "");
  const dirty = photos !== game.photos_per_participant || message !== (game.message ?? "");
  return (
    <section className="card">
      <h2>Game settings</h2>
      <div className="form-grid">
        <label>
          Photos per participant
          <select value={photos} onChange={(e) => setPhotos(Number(e.target.value))} data-testid="photos-per-participant">
            {Array.from({ length: 10 }, (_, i) => i + 1).map((n) => (
              <option key={n} value={n}>
                {n}
              </option>
            ))}
          </select>
        </label>
        <label className="span-2">
          Message to participants (optional, included in emails)
          <textarea
            rows={3}
            maxLength={1000}
            value={message}
            placeholder="e.g. Dig out your old holiday photos! Childhood pics welcome."
            onChange={(e) => setMessage(e.target.value)}
          />
        </label>
      </div>
      <button
        className="btn"
        disabled={!dirty}
        onClick={() =>
          run(
            () => api(`/games/${game.id}`, { method: "PATCH", body: { photos_per_participant: photos, message } }),
            "Settings saved",
          )
        }
      >
        Save settings
      </button>
    </section>
  );
}

function emailLabel(p: ParticipantView): { text: string; tone: string } | null {
  const e = p.last_email;
  if (!e) return null;
  const kind = e.kind === "invite" ? "Invite" : e.kind === "reminder" ? "Reminder" : e.kind === "play" ? "Play link" : "Email";
  if (e.status === "sent") return { text: `${kind} sent ${timeAgo(e.at)}`, tone: "muted" };
  if (e.status === "failed") return { text: `${kind} failed: ${e.error ?? "unknown error"}`, tone: "danger" };
  return { text: `${kind} queued${e.error ? ` (${e.error})` : ""}`, tone: "muted" };
}

function ParticipantsCard({ detail, run }: { detail: GameDetail; run: Run }) {
  const { organizer } = useAuth();
  const { game, participants } = detail;
  const [entries, setEntries] = useState("");
  const [feedback, setFeedback] = useState<string | null>(null);
  const canEdit = game.status === "draft" || game.status === "collecting";

  const add = async (e: FormEvent) => {
    e.preventDefault();
    setFeedback(null);
    try {
      const res = await api<{ added: number; duplicates: string[]; invalid: string[] }>(
        `/games/${game.id}/participants`,
        { body: { entries } },
      );
      const parts = [`${plural(res.added, "participant")} added`];
      if (game.status === "collecting" && res.added > 0) parts.push("invitations are on their way");
      if (res.duplicates.length) parts.push(`${res.duplicates.length} already in the list`);
      if (res.invalid.length) parts.push(`ignored invalid: ${res.invalid.join(", ")}`);
      setFeedback(parts.join(" · "));
      setEntries(res.invalid.join("\n"));
      await run(async () => {});
    } catch (err) {
      setFeedback(errorMessage(err));
    }
  };

  return (
    <section className="card" data-testid="participants-card">
      <div className="card-head">
        <h2>Participants ({participants.length})</h2>
        {participants.length > 0 && (
          <span className="muted small">
            {detail.stats.ready} ready · {participants.filter((p) => p.status === "partial").length} partial ·{" "}
            {participants.filter((p) => p.status === "not_started").length} not started
          </span>
        )}
      </div>
      {canEdit && (
        <form onSubmit={add} className="add-participants">
          <textarea
            rows={3}
            value={entries}
            onChange={(e) => setEntries(e.target.value)}
            placeholder={"Paste emails, one per line or comma separated:\nalice@company.com, Bob Martin <bob@company.com>"}
            data-testid="participants-input"
          />
          <button className="btn" disabled={!entries.trim()}>
            Add participants
          </button>
          {feedback && <p className="hint">{feedback}</p>}
        </form>
      )}
      {participants.length > 0 && (
        <div className="table-wrap">
          <table className="table">
            <thead>
              <tr>
                <th>Participant</th>
                <th>Status</th>
                <th>Photos</th>
                <th>Email</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {participants.map((p) => {
                const email = emailLabel(p);
                return (
                  <tr key={p.id} data-testid="participant-row">
                    <td>
                      <div className="strong">
                        {p.name || p.email}
                        {organizer && p.email.toLowerCase() === organizer.email.toLowerCase() && (
                          <span className="chip muted you-chip">you</span>
                        )}
                      </div>
                      {p.name && <div className="muted small">{p.email}</div>}
                    </td>
                    <td>
                      <ParticipantStatusChip status={p.status} done={p.done} />
                    </td>
                    <td>
                      {p.photos}/{game.photos_per_participant}
                    </td>
                    <td className={`small ${email?.tone ?? "muted"}`}>{email?.text ?? (game.status === "draft" ? "Not invited yet" : "—")}</td>
                    <td className="actions">
                      <CopyButton text={p.collect_url} label="Copy link" testId="copy-collect-link" />
                      {game.status === "collecting" && (
                        <button
                          className="btn small ghost"
                          onClick={() => run(() => api(`/games/${game.id}/participants/${p.id}/invite`, { body: {} }), "Invitation re-sent")}
                        >
                          Resend
                        </button>
                      )}
                      {canEdit && (
                        <button
                          className="btn small ghost danger"
                          onClick={() => {
                            if (confirm(`Remove ${p.email}${p.photos ? ` and their ${plural(p.photos, "photo")}` : ""}?`))
                              run(() => api(`/games/${game.id}/participants/${p.id}`, { method: "DELETE" }), "Participant removed");
                          }}
                        >
                          Remove
                        </button>
                      )}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
      {game.status !== "draft" && (
        <p className="hint">
          Each participant has a personal upload link. If an email doesn't arrive, copy the link and send it on Slack or Teams.
        </p>
      )}
    </section>
  );
}

function reminderPreview(deadline: Date, days: number): Date {
  const d = new Date(deadline);
  d.setDate(d.getDate() - days);
  d.setHours(10, 0, 0, 0);
  const latest = new Date(deadline.getTime() - 2 * 3600_000);
  return d < latest ? d : latest;
}

function ScheduleFields({
  deadline,
  setDeadline,
  reminders,
  setReminders,
}: {
  deadline: string;
  setDeadline: (v: string) => void;
  reminders: number[];
  setReminders: (v: number[]) => void;
}) {
  const [custom, setCustom] = useState("");
  const tz = browserTimeZone();
  const deadlineDate = deadline ? new Date(deadline) : null;
  const toggle = (d: number) =>
    setReminders(reminders.includes(d) ? reminders.filter((x) => x !== d) : [...reminders, d].sort((a, b) => b - a));
  const options = Array.from(new Set([...REMINDER_PRESETS, ...reminders])).sort((a, b) => b - a);
  return (
    <div className="schedule">
      <label>
        Deadline <span className="muted small">({tz})</span>
        <input
          type="datetime-local"
          value={deadline}
          min={toLocalInput(new Date())}
          onChange={(e) => setDeadline(e.target.value)}
          required
          data-testid="deadline-input"
        />
      </label>
      <div>
        <span className="label">Reminder emails (only to participants who aren't ready yet)</span>
        <div className="chips">
          {options.map((d) => (
            <button
              type="button"
              key={d}
              className={`chip-toggle ${reminders.includes(d) ? "on" : ""}`}
              onClick={() => toggle(d)}
              data-testid={`reminder-${d}`}
            >
              D-{d}
            </button>
          ))}
          <span className="custom-reminder">
            D-
            <input
              type="number"
              min={0}
              max={30}
              value={custom}
              onChange={(e) => setCustom(e.target.value)}
              placeholder="n"
              aria-label="Custom reminder: days before the deadline"
            />
            <button
              type="button"
              className="btn small ghost"
              disabled={custom === "" || Number(custom) < 0 || Number(custom) > 30}
              onClick={() => {
                const n = Number(custom);
                if (!reminders.includes(n)) setReminders([...reminders, n].sort((a, b) => b - a));
                setCustom("");
              }}
            >
              Add
            </button>
          </span>
        </div>
        {deadlineDate && reminders.length > 0 && (
          <ul className="reminder-preview">
            {reminders.map((d) => {
              const at = reminderPreview(deadlineDate, d);
              return (
                <li key={d} className={at < new Date() ? "muted strike" : ""}>
                  D-{d}: {formatDateTime(at.toISOString())}
                  {at < new Date() ? " (already past, skipped)" : ""}
                </li>
              );
            })}
          </ul>
        )}
      </div>
    </div>
  );
}

function defaultDeadline(): string {
  const d = new Date();
  d.setDate(d.getDate() + 7);
  d.setHours(18, 0, 0, 0);
  return toLocalInput(d);
}

function StartCollectingCard({ detail, run }: { detail: GameDetail; run: Run }) {
  const { game, participants } = detail;
  const [deadline, setDeadline] = useState(defaultDeadline());
  const [reminders, setReminders] = useState<number[]>([3, 1]);
  const start = () =>
    run(
      () =>
        api(`/games/${game.id}/start`, {
          body: {
            deadline_at: new Date(deadline).toISOString(),
            timezone: browserTimeZone(),
            reminder_days: reminders,
          },
        }),
      "Invitations sent!",
    );
  return (
    <section className="card highlight">
      <h2>Start collecting photos</h2>
      <p>
        Each participant gets an email asking for{" "}
        <strong>{plural(game.photos_per_participant, "photo")} of themselves</strong> with the year and the exact place.
        Every email says how many participants still haven't finished.
      </p>
      <ScheduleFields deadline={deadline} setDeadline={setDeadline} reminders={reminders} setReminders={setReminders} />
      <button
        className="btn primary"
        disabled={participants.length === 0 || !deadline}
        onClick={start}
        data-testid="start-collecting"
      >
        Send {plural(participants.length, "invitation")} & start collecting
      </button>
      {participants.length === 0 && <p className="hint">Add participants first.</p>}
    </section>
  );
}

function CollectionCard({ detail, run }: { detail: GameDetail; run: Run }) {
  const { game, stats, reminders } = detail;
  const [editing, setEditing] = useState(false);
  const [notify, setNotify] = useState(true);
  const [deadline, setDeadline] = useState(game.deadline_at ? toLocalInput(new Date(game.deadline_at)) : defaultDeadline());
  const [days, setDays] = useState<number[]>(Array.from(new Set(reminders.map((r) => r.days_before))));
  const deadlinePassed = game.deadline_at ? new Date(game.deadline_at) < new Date() : false;
  return (
    <section className="card highlight" data-testid="collection-card">
      <div className="card-head">
        <h2>Collecting photos</h2>
        <button className="btn small ghost" onClick={() => setEditing(true)}>
          Edit deadline & reminders
        </button>
      </div>
      <ProgressBar
        value={stats.ready}
        max={stats.total}
        label={
          <span data-testid="collection-progress">
            <strong>
              {stats.ready}/{stats.total}
            </strong>{" "}
            participants ready · {plural(stats.photos, "photo")} collected
          </span>
        }
      />
      <p>
        Deadline: <strong>{detail.deadline_label}</strong>
        {deadlinePassed && <span className="chip warn">passed: late uploads still accepted</span>}
      </p>
      {reminders.length > 0 && (
        <ul className="reminder-list">
          {reminders.map((r) => (
            <li key={r.id} className={r.status}>
              <span className="chip muted">D-{r.days_before}</span> {formatDateTime(r.send_at)} ·{" "}
              {r.status === "sent"
                ? `sent to ${plural(r.recipients ?? 0, "participant")}`
                : r.status === "skipped"
                  ? "skipped"
                  : "scheduled"}
            </li>
          ))}
        </ul>
      )}
      {detail.pending_emails > 0 && <p className="hint">{plural(detail.pending_emails, "email")} waiting to be sent…</p>}
      <div className="ready-box">
        <h3>Ready to play?</h3>
        <p>
          Marking the game as ready closes uploads and creates the play link.{" "}
          {stats.pending > 0 && <strong>{stats.pending} participant(s) haven't finished yet.</strong>}
        </p>
        <label className="checkbox">
          <input type="checkbox" checked={notify} onChange={(e) => setNotify(e.target.checked)} data-testid="notify-participants" />
          Also email the play link to all participants
        </label>
        <button
          className="btn primary"
          disabled={stats.photos === 0}
          data-testid="mark-ready"
          onClick={() => {
            if (stats.pending === 0 || confirm(`${stats.pending} participant(s) haven't finished. Start the game anyway?`))
              run(() => api(`/games/${game.id}/ready`, { body: { notify_participants: notify } }), "The game is ready!");
          }}
        >
          Mark game as ready
        </button>
        {stats.photos === 0 && <p className="hint">You need at least one photo.</p>}
      </div>
      {editing && (
        <Modal title="Deadline & reminders" onClose={() => setEditing(false)}>
          <ScheduleFields deadline={deadline} setDeadline={setDeadline} reminders={days} setReminders={setDays} />
          <button
            className="btn primary"
            onClick={async () => {
              await run(
                () =>
                  api(`/games/${game.id}/schedule`, {
                    method: "PUT",
                    body: { deadline_at: new Date(deadline).toISOString(), timezone: browserTimeZone(), reminder_days: days },
                  }),
                "Schedule updated",
              );
              setEditing(false);
            }}
          >
            Save schedule
          </button>
        </Modal>
      )}
    </section>
  );
}

function PlayCard({ detail, run }: { detail: GameDetail; run: Run }) {
  const { game, play_url, leaderboard_url } = detail;
  const [qrBig, setQrBig] = useState(false);
  if (!play_url) return null;
  return (
    <section className="card highlight play-card" data-testid="play-card">
      <div className="play-link-box">
        <div>
          <h2>{game.status === "finished" ? "Game ended" : "The game is open!"}</h2>
          <p>Share this link (or the QR code) with the players. Each team joins with a team name.</p>
          <div className="play-link">
            <a href={play_url} target="_blank" rel="noreferrer" data-testid="play-url">
              {play_url}
            </a>
            <CopyButton text={play_url} label="Copy link" />
          </div>
          <div className="row wrap">
            {leaderboard_url && (
              <a className="btn" href={leaderboard_url} target="_blank" rel="noreferrer">
                Open leaderboard (projector)
              </a>
            )}
            {game.status === "ready" && (
              <button className="btn ghost" onClick={() => run(() => api(`/games/${game.id}/notify-play`, { body: {} }), "Play link emailed to participants")}>
                Email link to participants
              </button>
            )}
          </div>
        </div>
        <button className="qr" onClick={() => setQrBig(true)} aria-label="Show QR code full screen">
          <QRCodeSVG value={play_url} size={148} marginSize={1} />
        </button>
      </div>
      <div className="row wrap game-controls">
        {game.status === "ready" && (
          <>
            <label className="checkbox">
              <input
                type="checkbox"
                checked={game.joining_closed}
                onChange={(e) => run(() => api(`/games/${game.id}/joining`, { body: { closed: e.target.checked } }))}
              />
              Close joining for new teams
            </label>
            <button
              className="btn"
              data-testid="end-game"
              onClick={() => {
                if (confirm("End the game? Teams can't submit more guesses, and all answers are revealed on the leaderboard."))
                  run(() => api(`/games/${game.id}/finish`, { body: {} }), "Game ended: answers revealed");
              }}
            >
              End game & reveal answers
            </button>
            {detail.teams.length === 0 && (
              <button className="btn ghost" onClick={() => run(() => api(`/games/${game.id}/reopen`, { body: {} }), "Collection reopened")}>
                Back to collecting
              </button>
            )}
          </>
        )}
        {game.status === "finished" && (
          <button className="btn ghost" onClick={() => run(() => api(`/games/${game.id}/resume`, { body: {} }), "Game resumed")}>
            Resume game
          </button>
        )}
      </div>
      {qrBig && (
        <Modal title={game.name} onClose={() => setQrBig(false)}>
          <div className="qr-big">
            <QRCodeSVG value={play_url} size={420} marginSize={2} />
            <p>{play_url}</p>
          </div>
        </Modal>
      )}
    </section>
  );
}

function TeamsCard({ detail, run }: { detail: GameDetail; run: Run }) {
  const { game, teams } = detail;
  return (
    <section className="card">
      <h2>Teams ({teams.length})</h2>
      {teams.length === 0 ? (
        <p className="muted">No team has joined yet.</p>
      ) : (
        <div className="table-wrap">
          <table className="table">
            <thead>
              <tr>
                <th>Team</th>
                <th>PIN</th>
                <th>Progress</th>
                <th>Score</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {teams.map((t) => (
                <tr key={t.id}>
                  <td className="strong">{t.name}</td>
                  <td>
                    <code>{t.pin}</code>
                  </td>
                  <td>{t.finished_at ? "Finished" : `${t.rounds_played} played`}</td>
                  <td className="strong">{t.total_score.toLocaleString("en-US")}</td>
                  <td className="actions">
                    <button
                      className="btn small ghost danger"
                      onClick={() => confirm(`Delete team ${t.name} and its guesses?`) && run(() => api(`/games/${game.id}/teams/${t.id}`, { method: "DELETE" }), "Team deleted")}
                    >
                      Delete
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      <p className="hint">A team can rejoin from another device with its name and PIN.</p>
    </section>
  );
}

function PhotosCard({ detail, photos, run }: { detail: GameDetail; photos: OrganizerPhoto[]; run: Run }) {
  const { game } = detail;
  const [open, setOpen] = useState<number | null>(null);
  const current = open !== null ? photos[open] : null;
  const markers = useMemo(
    () => (current ? [{ id: current.id, lng: current.lng, lat: current.lat, color: "#e8a33d" }] : []),
    [current],
  );
  return (
    <section className="card">
      <h2>Photos ({photos.length})</h2>
      {photos.length === 0 ? (
        <p className="muted">No photos yet.</p>
      ) : (
        <div className="photo-grid">
          {photos.map((p, i) => (
            <figure key={p.id} className="photo-tile" data-testid="organizer-photo">
              <button className="photo-thumb" onClick={() => setOpen(i)} aria-label="Open photo">
                <img src={`/api/images/${p.id}/thumb`} alt="" loading="lazy" />
              </button>
              <figcaption>
                <strong>{p.year}</strong> · {p.participant_name || p.participant_email}
                {p.caption && <div className="muted small">{p.caption}</div>}
              </figcaption>
              <button
                className="link-btn danger small"
                onClick={() => confirm("Delete this photo? Scores for it are removed too.") && run(() => api(`/games/${game.id}/photos/${p.id}`, { method: "DELETE" }), "Photo deleted")}
              >
                Delete
              </button>
            </figure>
          ))}
        </div>
      )}
      {current && open !== null && (
        <Lightbox
          src={`/api/images/${current.id}`}
          alt="Photo"
          onClose={() => setOpen(null)}
          onPrev={open > 0 ? () => setOpen(open - 1) : undefined}
          onNext={open < photos.length - 1 ? () => setOpen(open + 1) : undefined}
        >
          <h3>{current.year}</h3>
          <p>
            by <strong>{current.participant_name || current.participant_email}</strong>
          </p>
          {current.caption && <p className="muted">{current.caption}</p>}
          <MapView className="side-map" markers={markers} fitTo={[[current.lng, current.lat]]} initialZoom={3} initialCenter={[current.lng, current.lat]} />
        </Lightbox>
      )}
    </section>
  );
}

function DangerZone({ detail }: { detail: GameDetail }) {
  const navigate = useNavigate();
  return (
    <section className="card danger-zone">
      <h2>Danger zone</h2>
      <button
        className="btn danger"
        onClick={async () => {
          if (!confirm(`Delete “${detail.game.name}” with all its photos and scores? This can't be undone.`)) return;
          try {
            await api(`/games/${detail.game.id}`, { method: "DELETE" });
            navigate("/dashboard");
          } catch (e) {
            toast(errorMessage(e), "error");
          }
        }}
      >
        Delete game
      </button>
    </section>
  );
}
