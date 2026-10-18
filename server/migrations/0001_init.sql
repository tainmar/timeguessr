-- Organizers sign in with a magic link; participants and teams use opaque tokens.

CREATE TABLE organizers (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    email text NOT NULL,
    name text,
    -- 'all' | 'important' | 'none': which notifications are also sent by email
    email_notifications text NOT NULL DEFAULT 'important'
        CHECK (email_notifications IN ('all', 'important', 'none')),
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX organizers_email_key ON organizers (lower(email));

CREATE TABLE login_tokens (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    email text NOT NULL,
    token_hash text NOT NULL UNIQUE,
    ip text,
    expires_at timestamptz NOT NULL,
    used_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX login_tokens_email_idx ON login_tokens (lower(email), created_at);
CREATE INDEX login_tokens_ip_idx ON login_tokens (ip, created_at);

CREATE TABLE sessions (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    organizer_id uuid NOT NULL REFERENCES organizers (id) ON DELETE CASCADE,
    token_hash text NOT NULL UNIQUE,
    expires_at timestamptz NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE games (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    organizer_id uuid NOT NULL REFERENCES organizers (id) ON DELETE CASCADE,
    name text NOT NULL,
    status text NOT NULL DEFAULT 'draft'
        CHECK (status IN ('draft', 'collecting', 'ready', 'finished')),
    photos_per_participant int NOT NULL DEFAULT 1
        CHECK (photos_per_participant BETWEEN 1 AND 10),
    message text,
    timezone text NOT NULL DEFAULT 'UTC',
    deadline_at timestamptz,
    collection_started_at timestamptz,
    ready_at timestamptz,
    finished_at timestamptz,
    play_code text UNIQUE,
    joining_closed boolean NOT NULL DEFAULT false,
    deadline_notified_at timestamptz,
    all_ready_notified_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX games_organizer_idx ON games (organizer_id, created_at DESC);

CREATE TABLE participants (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    game_id uuid NOT NULL REFERENCES games (id) ON DELETE CASCADE,
    email text NOT NULL,
    name text,
    token text NOT NULL UNIQUE,
    -- set when the participant declares they won't upload more photos
    done_at timestamptz,
    ready_notified_at timestamptz,
    invited_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX participants_game_email_key ON participants (game_id, lower(email));

CREATE TABLE photos (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    game_id uuid NOT NULL REFERENCES games (id) ON DELETE CASCADE,
    participant_id uuid NOT NULL REFERENCES participants (id) ON DELETE CASCADE,
    year int NOT NULL,
    lat double precision NOT NULL,
    lng double precision NOT NULL,
    caption text,
    width int NOT NULL,
    height int NOT NULL,
    image bytea NOT NULL,
    thumb bytea NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX photos_game_idx ON photos (game_id, created_at);
CREATE INDEX photos_participant_idx ON photos (participant_id);

CREATE TABLE reminders (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    game_id uuid NOT NULL REFERENCES games (id) ON DELETE CASCADE,
    days_before int NOT NULL,
    send_at timestamptz NOT NULL,
    status text NOT NULL DEFAULT 'scheduled' CHECK (status IN ('scheduled', 'sent', 'skipped')),
    recipients int,
    processed_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX reminders_due_idx ON reminders (status, send_at);

CREATE TABLE teams (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    game_id uuid NOT NULL REFERENCES games (id) ON DELETE CASCADE,
    name text NOT NULL,
    token text NOT NULL UNIQUE,
    pin text NOT NULL,
    photo_order uuid[] NOT NULL,
    finished_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX teams_game_name_key ON teams (game_id, lower(name));

CREATE TABLE guesses (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    team_id uuid NOT NULL REFERENCES teams (id) ON DELETE CASCADE,
    photo_id uuid NOT NULL REFERENCES photos (id) ON DELETE CASCADE,
    year int NOT NULL,
    lat double precision NOT NULL,
    lng double precision NOT NULL,
    distance_km double precision NOT NULL,
    year_score int NOT NULL,
    location_score int NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (team_id, photo_id)
);
CREATE INDEX guesses_photo_idx ON guesses (photo_id);

CREATE TABLE notifications (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    organizer_id uuid NOT NULL REFERENCES organizers (id) ON DELETE CASCADE,
    game_id uuid REFERENCES games (id) ON DELETE CASCADE,
    kind text NOT NULL,
    title text NOT NULL,
    body text NOT NULL,
    link text,
    read_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX notifications_organizer_idx ON notifications (organizer_id, created_at DESC);

-- Transactional outbox: every email goes through here and is sent by the worker.
CREATE TABLE emails (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    organizer_id uuid REFERENCES organizers (id) ON DELETE SET NULL,
    game_id uuid REFERENCES games (id) ON DELETE SET NULL,
    participant_id uuid REFERENCES participants (id) ON DELETE SET NULL,
    kind text NOT NULL,
    to_email text NOT NULL,
    reply_to text,
    subject text NOT NULL,
    html text NOT NULL,
    text_body text NOT NULL,
    status text NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'sending', 'sent', 'failed')),
    attempts int NOT NULL DEFAULT 0,
    next_attempt_at timestamptz NOT NULL DEFAULT now(),
    locked_until timestamptz,
    provider_id text,
    last_error text,
    created_at timestamptz NOT NULL DEFAULT now(),
    sent_at timestamptz
);
CREATE INDEX emails_queue_idx ON emails (status, next_attempt_at);
CREATE INDEX emails_participant_idx ON emails (participant_id, created_at DESC);
CREATE INDEX emails_organizer_idx ON emails (organizer_id, created_at);

-- Collection progress of each participant: ready when all photos are in or they declared done.
CREATE VIEW participant_progress AS
SELECT p.id,
       p.game_id,
       p.email,
       p.name,
       p.token,
       p.done_at,
       p.ready_notified_at,
       p.invited_at,
       p.created_at,
       count(ph.id) AS photo_count,
       (p.done_at IS NOT NULL OR count(ph.id) >= g.photos_per_participant) AS ready
FROM participants p
JOIN games g ON g.id = p.game_id
LEFT JOIN photos ph ON ph.participant_id = p.id
GROUP BY p.id, g.photos_per_participant;
