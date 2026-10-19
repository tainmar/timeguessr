// Typed client for the TimeGuessr API.

export class ApiError extends Error {
  constructor(
    message: string,
    public status: number,
  ) {
    super(message);
  }
}

type Options = {
  method?: string;
  body?: unknown;
  form?: FormData;
  teamToken?: string | null;
  signal?: AbortSignal;
};

export async function api<T>(path: string, opts: Options = {}): Promise<T> {
  const headers: Record<string, string> = {};
  let body: BodyInit | undefined;
  if (opts.form) {
    body = opts.form;
  } else if (opts.body !== undefined) {
    headers["Content-Type"] = "application/json";
    body = JSON.stringify(opts.body);
  }
  if (opts.teamToken) headers["X-Team-Token"] = opts.teamToken;
  let res: Response;
  try {
    res = await fetch(`/api${path}`, {
      method: opts.method ?? (body ? "POST" : "GET"),
      headers,
      body,
      credentials: "same-origin",
      signal: opts.signal,
    });
  } catch (e) {
    if ((e as Error).name === "AbortError") throw e;
    throw new ApiError("Network error: check your connection and try again.", 0);
  }
  const text = await res.text();
  let data: unknown = null;
  try {
    data = text ? JSON.parse(text) : null;
  } catch {
    data = null;
  }
  if (!res.ok) {
    const message =
      (data as { error?: string } | null)?.error ??
      (res.status === 413 ? "This file is too big." : `Request failed (${res.status})`);
    throw new ApiError(message, res.status);
  }
  return data as T;
}

export type GameStatus = "draft" | "collecting" | "ready" | "finished";
export type ParticipantStatus = "not_started" | "partial" | "ready";

export interface Organizer {
  id: string;
  email: string;
  name: string | null;
  email_notifications: "all" | "important" | "none";
}

export interface GameListItem {
  id: string;
  name: string;
  status: GameStatus;
  photos_per_participant: number;
  deadline_at: string | null;
  timezone: string;
  created_at: string;
  participants: number;
  ready_participants: number;
  photos: number;
  teams: number;
}

export interface Game {
  id: string;
  name: string;
  status: GameStatus;
  photos_per_participant: number;
  message: string | null;
  timezone: string;
  deadline_at: string | null;
  collection_started_at: string | null;
  ready_at: string | null;
  finished_at: string | null;
  play_code: string | null;
  joining_closed: boolean;
  created_at: string;
}

export interface ParticipantView {
  id: string;
  email: string;
  name: string | null;
  status: ParticipantStatus;
  photos: number;
  done: boolean;
  collect_url: string;
  invited_at: string | null;
  last_email: { kind: string; status: string; error: string | null; at: string } | null;
}

export interface ReminderView {
  id: string;
  days_before: number;
  send_at: string;
  status: "scheduled" | "sent" | "skipped";
  recipients: number | null;
}

export interface TeamView {
  id: string;
  name: string;
  pin: string;
  created_at: string;
  finished_at: string | null;
  rounds_played: number;
  total_score: number;
}

export interface CollectionStats {
  total: number;
  ready: number;
  pending: number;
  photos: number;
}

export interface GameDetail {
  game: Game;
  play_url: string | null;
  leaderboard_url: string | null;
  deadline_label: string | null;
  participants: ParticipantView[];
  reminders: ReminderView[];
  teams: TeamView[];
  stats: CollectionStats;
  pending_emails: number;
  max_participants: number;
}

export interface OrganizerPhoto {
  id: string;
  participant_id: string;
  participant_name: string | null;
  participant_email: string;
  year: number;
  lat: number;
  lng: number;
  width: number;
  height: number;
  created_at: string;
}

export interface NotificationItem {
  id: string;
  game_id: string | null;
  kind: string;
  title: string;
  body: string;
  link: string | null;
  read_at: string | null;
  created_at: string;
}

export interface CollectPhoto {
  id: string;
  year: number;
  lat: number;
  lng: number;
  width: number;
  height: number;
  created_at: string;
}

export interface CollectView {
  game: {
    name: string;
    status: GameStatus;
    organizer: string;
    message: string | null;
    photos_per_participant: number;
    deadline_at: string | null;
    deadline_label: string | null;
    deadline_passed: boolean;
    play_url: string | null;
  };
  participant: {
    name: string | null;
    email: string;
    status: ParticipantStatus;
    done: boolean;
    photo_count: number;
  };
  photos: CollectPhoto[];
  stats: CollectionStats;
  can_edit: boolean;
  can_upload: boolean;
  min_year: number;
  max_year: number;
}

export interface PlayInfo {
  name: string;
  status: GameStatus;
  organizer: string;
  photo_count: number;
  team_count: number;
  year_min: number;
  year_max: number;
  joining_open: boolean;
}

export interface TeamRef {
  id: string;
  name: string;
  pin: string;
}

export interface PlayState {
  team: TeamRef;
  game: { name: string; status: GameStatus; year_min: number; year_max: number };
  round: { number: number; total: number; photo_id: string; image_url: string } | null;
  next_image_url: string | null;
  rounds_played: number;
  total_rounds: number;
  total_score: number;
  finished: boolean;
}

export interface Answer {
  year: number;
  lat: number;
  lng: number;
  owner: string;
}

export interface Reveal {
  photo_id: string;
  image_url: string;
  guess: { year: number; lat: number; lng: number };
  answer: Answer;
  distance_km: number;
  year_score: number;
  location_score: number;
  score: number;
  total_score: number;
  round_number: number | null;
  total_rounds: number;
  finished: boolean;
}

export interface LeaderboardPhoto {
  id: string;
  number: number;
  image_url: string;
  thumb_url: string;
  answer?: Answer;
  guesses?: {
    team_id: string;
    team_name: string;
    year: number;
    lat: number;
    lng: number;
    distance_km: number;
    score: number;
  }[];
}

export interface LeaderboardTeam {
  id: string;
  name: string;
  total: number;
  rounds_played: number;
  rounds_total: number;
  finished: boolean;
  finished_at: string | null;
  scores: Record<string, { year_score: number; location_score: number; score: number }>;
}

export interface Leaderboard {
  game: { name: string; status: GameStatus };
  photos: LeaderboardPhoto[];
  teams: LeaderboardTeam[];
  reveal_answers: boolean;
  viewer_team_id: string | null;
  generated_at: string;
}

const TEAM_KEY = (code: string) => `tg.team.${code}`;

export function loadTeamToken(code: string): string | null {
  try {
    return localStorage.getItem(TEAM_KEY(code));
  } catch {
    return null;
  }
}

export function saveTeamToken(code: string, token: string | null) {
  try {
    if (token) localStorage.setItem(TEAM_KEY(code), token);
    else localStorage.removeItem(TEAM_KEY(code));
  } catch {
    /* private mode: the token only lives for this page */
  }
}
