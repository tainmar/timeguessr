import { useState, type FormEvent } from "react";
import { api, ApiError, type Organizer } from "../api";
import { useAuth } from "../auth";
import { OrganizerLayout } from "../components/Layout";
import { toast } from "../components/ui";

export default function Settings() {
  const { organizer, setOrganizer } = useAuth();
  const [name, setName] = useState(organizer?.name ?? "");
  const [pref, setPref] = useState(organizer?.email_notifications ?? "important");
  const [busy, setBusy] = useState(false);

  const save = async (e: FormEvent) => {
    e.preventDefault();
    setBusy(true);
    try {
      const updated = await api<Organizer>("/me", { method: "PATCH", body: { name, email_notifications: pref } });
      setOrganizer(updated);
      toast("Settings saved");
    } catch (err) {
      toast(err instanceof ApiError ? err.message : "Could not save", "error");
    } finally {
      setBusy(false);
    }
  };

  return (
    <OrganizerLayout>
      <div className="page-head">
        <h1>Settings</h1>
      </div>
      <form className="card narrow" onSubmit={save}>
        <label htmlFor="org-name">Your name (shown to participants in emails)</label>
        <input id="org-name" maxLength={80} value={name} onChange={(e) => setName(e.target.value)} placeholder={organizer?.email} />
        <fieldset className="radio-group">
          <legend>Email notifications</legend>
          <p className="hint">You always get notifications in the app (bell icon). Choose which ones are also emailed to you.</p>
          {[
            ["important", "Important only: everyone is ready, deadline reached (recommended)"],
            ["all", "Everything: every upload, every ready participant, every finished team"],
            ["none", "None: in-app only"],
          ].map(([value, label]) => (
            <label key={value} className="radio">
              <input type="radio" name="pref" value={value} checked={pref === value} onChange={() => setPref(value as typeof pref)} />
              {label}
            </label>
          ))}
        </fieldset>
        <button className="btn primary" disabled={busy}>
          Save
        </button>
      </form>
    </OrganizerLayout>
  );
}
