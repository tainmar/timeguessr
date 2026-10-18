import { useEffect, useRef, useState } from "react";
import { Link, useNavigate } from "react-router";
import { api, ApiError, type Organizer } from "../api";
import { useAuth } from "../auth";
import { PublicLayout } from "../components/Layout";
import { Spinner } from "../components/ui";

export default function AuthVerify() {
  const navigate = useNavigate();
  const { setOrganizer } = useAuth();
  const [error, setError] = useState<string | null>(null);
  const started = useRef(false);

  useEffect(() => {
    if (started.current) return;
    started.current = true;
    const token = new URLSearchParams(window.location.hash.slice(1)).get("token");
    if (!token) {
      setError("This sign-in link is incomplete. Please copy the whole link from the email.");
      return;
    }
    // Drop the token from the address bar and history.
    window.history.replaceState(null, "", "/auth/verify");
    api<Organizer>("/auth/verify", { body: { token } })
      .then((organizer) => {
        setOrganizer(organizer);
        navigate("/dashboard", { replace: true });
      })
      .catch((e) => setError(e instanceof ApiError ? e.message : "Sign-in failed."));
  }, [navigate, setOrganizer]);

  return (
    <PublicLayout>
      <div className="card narrow center">
        {error ? (
          <>
            <h2>Sign-in failed</h2>
            <p>{error}</p>
            <Link to="/" className="btn primary">
              Request a new link
            </Link>
          </>
        ) : (
          <Spinner label="Signing you in…" />
        )}
      </div>
    </PublicLayout>
  );
}
