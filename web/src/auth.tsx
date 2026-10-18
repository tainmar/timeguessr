import { createContext, useCallback, useContext, useEffect, useRef, useState, type ReactNode } from "react";
import { api, type Organizer } from "./api";

interface AuthState {
  organizer: Organizer | null;
  loading: boolean;
  refresh: () => Promise<void>;
  setOrganizer: (o: Organizer | null) => void;
}

const AuthContext = createContext<AuthState>({
  organizer: null,
  loading: true,
  refresh: async () => {},
  setOrganizer: () => {},
});

export function AuthProvider({ children }: { children: ReactNode }) {
  const [organizer, setOrganizerState] = useState<Organizer | null>(null);
  const [loading, setLoading] = useState(true);
  // Every change bumps the version so a slow /me response can't overwrite a newer state
  // (e.g. the initial /me returning "signed out" after the magic link signed us in).
  const version = useRef(0);

  const setOrganizer = useCallback((o: Organizer | null) => {
    version.current += 1;
    setOrganizerState(o);
    setLoading(false);
  }, []);

  const refresh = useCallback(async () => {
    const v = ++version.current;
    try {
      const res = await api<{ organizer: Organizer | null }>("/me");
      if (v === version.current) setOrganizerState(res.organizer);
    } catch {
      if (v === version.current) setOrganizerState(null);
    } finally {
      if (v === version.current) setLoading(false);
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  return (
    <AuthContext.Provider value={{ organizer, loading, refresh, setOrganizer }}>{children}</AuthContext.Provider>
  );
}

export const useAuth = () => useContext(AuthContext);
