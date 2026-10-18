import { useCallback, useEffect, useRef, useState, type FormEvent } from "react";
import { useParams } from "react-router";
import { api, ApiError, type CollectPhoto, type CollectView } from "../api";
import { PublicLayout } from "../components/Layout";
import LocationPicker from "../components/LocationPicker";
import { ErrorBox, ParticipantStatusChip, ProgressBar, Spinner, toast } from "../components/ui";
import { plural } from "../lib/format";
import { prepareImage, readHints, type PhotoHints } from "../lib/image";

function errorMessage(e: unknown) {
  return e instanceof ApiError ? e.message : "Something went wrong.";
}

export default function Collect() {
  const { token } = useParams();
  const [view, setView] = useState<CollectView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);
  const [editing, setEditing] = useState<CollectPhoto | null>(null);

  const load = useCallback(async () => {
    try {
      const v = await api<CollectView>(`/collect/${token}`);
      setView(v);
      setError(null);
      return v;
    } catch (e) {
      setError(e instanceof ApiError && e.status === 404 ? "This link is not valid. Ask the organizer for your personal link." : errorMessage(e));
      return null;
    }
  }, [token]);

  useEffect(() => {
    load().then((v) => {
      if (v && v.photos.length === 0 && v.can_upload) setAdding(true);
    });
  }, [load]);

  if (!view) {
    return <PublicLayout>{error ? <ErrorBox error={error} onRetry={load} /> : <Spinner />}</PublicLayout>;
  }

  const { game, participant, stats } = view;
  const n = game.photos_per_participant;
  const remaining = n - participant.photo_count;

  const setDone = async (done: boolean) => {
    try {
      await api(`/collect/${token}/${done ? "done" : "undone"}`, { body: {} });
      toast(done ? "Thanks! You're all set." : "You can add more photos.");
      await load();
    } catch (e) {
      toast(errorMessage(e), "error");
    }
  };

  return (
    <PublicLayout>
      <section className="card collect-head">
        <p className="eyebrow">{game.organizer} invited you to</p>
        <h1 data-testid="collect-title">{game.name}</h1>
        {game.message && <blockquote className="organizer-message">{game.message}</blockquote>}
        <p>
          Share <strong>{plural(n, "photo")} of yourself</strong>. For each one, tell us the <strong>year</strong> and pin the{" "}
          <strong>exact place</strong> on the map. Teams will then try to guess when and where they were taken!
        </p>
        {game.deadline_label && (
          <p className={game.deadline_passed ? "warn-text" : ""}>
            ⏰ Deadline: <strong>{game.deadline_label}</strong>
            {game.deadline_passed && view.can_edit && " (passed, but you can still upload until the game starts)"}
          </p>
        )}
        <div className="collect-status">
          <ParticipantStatusChip status={participant.status} done={participant.done} />
          <ProgressBar
            value={participant.photo_count}
            max={n}
            label={<span data-testid="my-progress">{participant.photo_count} of {plural(n, "photo")} shared</span>}
          />
        </div>
        <p className="muted small" data-testid="pending-stats">
          {stats.pending === 0
            ? `All ${stats.total} participants are done.`
            : `${stats.pending} of ${stats.total} participants haven't finished uploading yet.`}
        </p>
      </section>

      {!view.can_edit && (
        <section className="card notice">
          {game.status === "draft" ? (
            <p>The collection hasn't started yet.</p>
          ) : (
            <>
              <h2>Uploads are closed</h2>
              <p>The organizer has started the game.</p>
              {game.play_url && (
                <a className="btn primary" href={game.play_url}>
                  Play now →
                </a>
              )}
            </>
          )}
        </section>
      )}

      {view.can_edit && <NameCard token={token!} view={view} onSaved={load} />}

      {view.photos.length > 0 && (
        <section className="card">
          <h2>Your photos</h2>
          <div className="my-photos">
            {view.photos.map((p) => (
              <figure key={p.id} className="my-photo" data-testid="my-photo">
                <img src={`/api/images/${p.id}/thumb`} alt="" />
                <figcaption>
                  <strong>{p.year}</strong>
                  {p.caption && <span className="muted"> · {p.caption}</span>}
                  <div className="muted small">
                    📍 {p.lat.toFixed(4)}, {p.lng.toFixed(4)}
                  </div>
                </figcaption>
                {view.can_edit && (
                  <div className="row">
                    <button className="btn small ghost" onClick={() => setEditing(p)}>
                      Edit
                    </button>
                    <button
                      className="btn small ghost danger"
                      onClick={async () => {
                        if (!confirm("Delete this photo?")) return;
                        try {
                          await api(`/collect/${token}/photos/${p.id}`, { method: "DELETE" });
                          await load();
                        } catch (e) {
                          toast(errorMessage(e), "error");
                        }
                      }}
                    >
                      Delete
                    </button>
                  </div>
                )}
              </figure>
            ))}
          </div>
        </section>
      )}

      {editing && (
        <PhotoForm
          key={editing.id}
          token={token!}
          view={view}
          existing={editing}
          onCancel={() => setEditing(null)}
          onSaved={async () => {
            setEditing(null);
            await load();
          }}
        />
      )}

      {view.can_upload && !editing && (
        adding ? (
          <PhotoForm
            // A fresh form (empty year, caption and pin) for every new photo.
            key={`new-${view.photos.length}`}
            token={token!}
            view={view}
            onCancel={view.photos.length > 0 ? () => setAdding(false) : undefined}
            onSaved={async () => {
              const v = await load();
              setAdding(!!v && v.can_upload && !v.participant.done);
              toast("Photo saved!");
            }}
          />
        ) : (
          !participant.done && (
            <button className="btn primary block" onClick={() => setAdding(true)} data-testid="add-photo">
              + Add photo {participant.photo_count + 1} of {n}
            </button>
          )
        )
      )}

      {view.can_edit && remaining > 0 && (
        <section className="card done-card">
          {participant.done ? (
            <>
              <p>
                ✅ You told us you're done with {plural(participant.photo_count, "photo")}. You won't get more reminders.
              </p>
              <button className="btn ghost" onClick={() => setDone(false)}>
                I changed my mind: add more photos
              </button>
            </>
          ) : (
            <>
              <p className="muted">
                Can't find {remaining === n ? "any photo" : `${plural(remaining, "more photo")}`}? Let the organizer know you're done:
              </p>
              <button className="btn ghost" onClick={() => setDone(true)} data-testid="im-done">
                I'm done, I don't want to share {participant.photo_count === 0 ? "any photo" : "more photos"}
              </button>
            </>
          )}
        </section>
      )}
      {view.can_edit && participant.status === "ready" && !participant.done && (
        <section className="card notice success" data-testid="all-done">
          <h2>🎉 All done, thank you!</h2>
          <p>You can still edit your photos until the organizer starts the game.</p>
        </section>
      )}
    </PublicLayout>
  );
}

function NameCard({ token, view, onSaved }: { token: string; view: CollectView; onSaved: () => void }) {
  const [name, setName] = useState(view.participant.name ?? "");
  const [saving, setSaving] = useState(false);
  const dirty = name.trim() !== (view.participant.name ?? "");
  const save = async (e?: FormEvent) => {
    e?.preventDefault();
    setSaving(true);
    try {
      await api(`/collect/${token}`, { method: "PATCH", body: { name } });
      toast("Name saved");
      onSaved();
    } catch (err) {
      toast(errorMessage(err), "error");
    } finally {
      setSaving(false);
    }
  };
  return (
    <form className="card name-card" onSubmit={save}>
      <label htmlFor="my-name">Your name (revealed to the players with your photos)</label>
      <div className="row">
        <input id="my-name" maxLength={60} value={name} onChange={(e) => setName(e.target.value)} placeholder="e.g. Alice M." data-testid="my-name" />
        <button className="btn" disabled={!dirty || saving || !name.trim()}>
          Save
        </button>
      </div>
    </form>
  );
}

function PhotoForm({
  token,
  view,
  existing,
  onCancel,
  onSaved,
}: {
  token: string;
  view: CollectView;
  existing?: CollectPhoto;
  onCancel?: () => void;
  onSaved: () => void;
}) {
  const [file, setFile] = useState<Blob | null>(null);
  const [preview, setPreview] = useState<string | null>(existing ? `/api/images/${existing.id}` : null);
  const [hints, setHints] = useState<PhotoHints | null>(null);
  const [year, setYear] = useState<string>(existing ? String(existing.year) : "");
  const [location, setLocation] = useState<{ lat: number; lng: number } | null>(
    existing ? { lat: existing.lat, lng: existing.lng } : null,
  );
  const [caption, setCaption] = useState(existing?.caption ?? "");
  const [preparing, setPreparing] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const fileInput = useRef<HTMLInputElement>(null);

  useEffect(() => {
    return () => {
      if (preview?.startsWith("blob:")) URL.revokeObjectURL(preview);
    };
  }, [preview]);

  const pick = async (f: File | undefined) => {
    if (!f) return;
    setError(null);
    setPreparing(true);
    try {
      const [prepared, h] = await Promise.all([prepareImage(f), readHints(f)]);
      setFile(prepared);
      setHints(h);
      setPreview(URL.createObjectURL(prepared));
    } catch {
      setError("We couldn't read this image. Try a JPEG or PNG.");
    } finally {
      setPreparing(false);
    }
  };

  const yearNum = Number(year);
  const yearValid = year !== "" && Number.isInteger(yearNum) && yearNum >= view.min_year && yearNum <= view.max_year;
  const needsName = !view.participant.name;
  const canSave = (existing || file) && yearValid && location && !saving && !preparing;

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!canSave || !location) return;
    setSaving(true);
    setError(null);
    try {
      if (existing) {
        await api(`/collect/${token}/photos/${existing.id}`, {
          method: "PATCH",
          body: { year: yearNum, lat: location.lat, lng: location.lng, caption },
        });
      } else {
        const form = new FormData();
        form.append("year", String(yearNum));
        form.append("lat", String(location.lat));
        form.append("lng", String(location.lng));
        form.append("caption", caption);
        form.append("file", file!, "photo.jpg");
        await api(`/collect/${token}/photos`, { form, method: "POST" });
      }
      onSaved();
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setSaving(false);
    }
  };

  return (
    <form className="card photo-form" onSubmit={submit} data-testid="photo-form">
      <h2>{existing ? "Edit photo" : `Photo ${view.participant.photo_count + 1} of ${view.game.photos_per_participant}`}</h2>
      {needsName && !existing && <p className="warn-text">Tip: add your name above so players know who's in the photo.</p>}

      <div className="photo-form-grid">
        <div>
          <span className="label">1. The photo</span>
          {preview ? (
            <div className="preview">
              <img src={preview} alt="Preview" />
              {!existing && (
                <button type="button" className="btn small ghost" onClick={() => fileInput.current?.click()}>
                  Choose another
                </button>
              )}
            </div>
          ) : (
            <button
              type="button"
              className="dropzone"
              onClick={() => fileInput.current?.click()}
              onDragOver={(e) => e.preventDefault()}
              onDrop={(e) => {
                e.preventDefault();
                pick(e.dataTransfer.files[0]);
              }}
            >
              {preparing ? "Preparing…" : "📷 Choose a photo of yourself (or drop it here)"}
            </button>
          )}
          <input
            ref={fileInput}
            type="file"
            accept="image/*,.heic,.heif"
            hidden
            onChange={(e) => pick(e.target.files?.[0])}
            data-testid="file-input"
          />
          <p className="hint">Scans of old prints are perfect. The file is resized, and its metadata is removed before players see it.</p>

          <label className="year-field">
            2. Year it was taken
            <input
              type="number"
              inputMode="numeric"
              min={view.min_year}
              max={view.max_year}
              value={year}
              onChange={(e) => setYear(e.target.value)}
              placeholder="e.g. 1998"
              required
              data-testid="year-input"
            />
          </label>
          {hints?.year && String(hints.year) !== year && (
            <button type="button" className="suggestion" onClick={() => setYear(String(hints.year))}>
              📅 The file says {hints.year}. Use it? (Careful with scans: that's the scan date.)
            </button>
          )}
          {year !== "" && !yearValid && (
            <p className="form-error">
              Enter a year between {view.min_year} and {view.max_year}.
            </p>
          )}

          <label>
            Caption (optional, shown after the guess)
            <input
              maxLength={200}
              value={caption}
              onChange={(e) => setCaption(e.target.value)}
              placeholder="e.g. My first concert"
              data-testid="caption-input"
            />
          </label>
        </div>
        <div>
          <span className="label">3. Where exactly?</span>
          <LocationPicker
            value={location}
            onChange={setLocation}
            suggestion={hints?.lat != null && hints?.lng != null ? { lat: hints.lat, lng: hints.lng } : null}
          />
          <p className="hint">Players will see this place after guessing. Avoid pinning your home address.</p>
        </div>
      </div>
      {error && <p className="form-error">{error}</p>}
      <div className="row">
        <button className="btn primary" disabled={!canSave} data-testid="save-photo">
          {saving ? "Uploading…" : existing ? "Save changes" : "Save photo"}
        </button>
        {onCancel && (
          <button type="button" className="btn ghost" onClick={onCancel}>
            Cancel
          </button>
        )}
        {!canSave && !saving && (
          <span className="muted small">
            {!existing && !file ? "Choose a photo" : !yearValid ? "Enter the year" : !location ? "Pin the place on the map" : ""}
          </span>
        )}
      </div>
    </form>
  );
}
