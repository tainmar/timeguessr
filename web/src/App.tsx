import { lazy, Suspense, type ReactNode } from "react";
import { BrowserRouter, Link, Navigate, Route, Routes } from "react-router";
import { AuthProvider, useAuth } from "./auth";
import { PublicLayout } from "./components/Layout";
import { Spinner, Toaster } from "./components/ui";
import AuthVerify from "./pages/AuthVerify";
import Dashboard from "./pages/Dashboard";
import Landing from "./pages/Landing";
import Settings from "./pages/Settings";

// Pages with maps are split out so the landing page stays light.
const Collect = lazy(() => import("./pages/Collect"));
const GamePage = lazy(() => import("./pages/GamePage"));
const Leaderboard = lazy(() => import("./pages/Leaderboard"));
const Play = lazy(() => import("./pages/Play"));

function RequireOrganizer({ children }: { children: ReactNode }) {
  const { organizer, loading } = useAuth();
  if (loading) return <Spinner />;
  if (!organizer) return <Navigate to="/" replace />;
  return <>{children}</>;
}

function NotFound() {
  return (
    <PublicLayout>
      <div className="card narrow center">
        <h1>Page not found</h1>
        <Link to="/" className="btn primary">
          Home
        </Link>
      </div>
    </PublicLayout>
  );
}

export default function App() {
  return (
    <BrowserRouter>
      <AuthProvider>
        <Suspense fallback={<Spinner />}>
        <Routes>
          <Route path="/" element={<Landing />} />
          <Route path="/auth/verify" element={<AuthVerify />} />
          <Route
            path="/dashboard"
            element={
              <RequireOrganizer>
                <Dashboard />
              </RequireOrganizer>
            }
          />
          <Route
            path="/settings"
            element={
              <RequireOrganizer>
                <Settings />
              </RequireOrganizer>
            }
          />
          <Route
            path="/games/:id"
            element={
              <RequireOrganizer>
                <GamePage />
              </RequireOrganizer>
            }
          />
          <Route path="/c/:token" element={<Collect />} />
          <Route path="/play/:code" element={<Play />} />
          <Route path="/play/:code/leaderboard" element={<Leaderboard />} />
          <Route path="*" element={<NotFound />} />
        </Routes>
        </Suspense>
        <Toaster />
      </AuthProvider>
    </BrowserRouter>
  );
}
