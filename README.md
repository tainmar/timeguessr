# TimeGuessr for teams

A self-hosted [TimeGuessr](https://timeguessr.com)-style game made of your team's own photos.
The organizer collects a few photos from each participant (with the year and the exact place).
Then teams guess **when** and **where** each one was taken.

Live at **https://timeguessr.procrastination.land**.

## How it works

1. **Organizer** signs in with a magic link (no password), creates a game and pastes participant emails
   (`alice@x.com` or `Alice Martin <alice@x.com>`).
2. **Start collecting**: pick a deadline and "D-n" reminders. Each participant gets a personal upload link
   by email, no account needed. Every email says how many participants still haven't finished.
3. **Participants** upload *n* photos of themselves. For each photo they enter the year and pin the exact place
   on an English OpenStreetMap map. The date and GPS stored in the file are offered as suggestions.
   A participant is **ready** when all *n* photos are in, or when they click "I'm done". Before that they are
   **partial**.
4. **Reminders** go out at 10:00 (organizer's time zone) n days before the deadline. They only go to
   participants who aren't ready yet. After the deadline, uploads stay open until the organizer starts the game.
5. **Organizer notifications** are sent in-app (bell) and/or by email: uploads, participants ready, everyone ready,
   deadline reached, teams finished, failed emails.
6. **Game ready** closes uploads and creates the play link and QR code, which can be emailed to every participant.
7. **Teams** join with a team name. Teammates can join from other phones with the team's 4-digit PIN.
   Each team gets the photos in its own random order. For each one they pick a year
   (slider from the oldest photo − 5 to this year) and a place on the map. Then they see the answer and their score.
8. **Leaderboard**: per-photo and total scores, refreshed every 10 s. Click any photo to zoom in
   (pinch, trackpad, wheel, double-tap). Answers stay hidden from teams still playing until they finish,
   or until the organizer clicks "End game & reveal answers".

### Scoring (max 1,000 points per photo)

| | Formula | Examples |
|---|---|---|
| Year (500) | `500 / (1 + (Δyears / 5)²)` | 0 → 500, 1 → 481, 2 → 431, 5 → 250, 10 → 100, 20 → 29 |
| Place (500) | `500 · (1 − ln(1 + d/0.5 km) / ln(1 + D/0.5 km))`, D = distance to the antipode (≈ 20,015 km) | 0 m → 500, 1 km → 448, 10 km → 356, 100 km → 250, 2,000 km → 109, antipode → **0** |

The place curve is logarithmic, so it keeps getting steeper as you get closer. It tells "right street" apart from
"right neighbourhood" just as well as "right country" from "right continent".

## Architecture

- `server/`: Rust (axum, sqlx, Postgres). REST API, background worker (email outbox and reminder scheduler)
  and static file serving. Photos are re-encoded server-side (EXIF stripped, so players can't read the answer
  from the file) and stored in Postgres.
- `web/`: React + TypeScript (Vite). MapLibre GL with [OpenFreeMap](https://openfreemap.org) tiles, labels forced to
  English. Place search via Nominatim.
- `e2e/`: Playwright end-to-end tests (full journey on desktop and phones, plus API edge cases).
- Emails go through a transactional outbox table and are sent by the worker via [Resend](https://resend.com),
  using idempotency keys and retries. The quota is respected.
- Notifications follow Laravel's notification design: each notification type declares the channels it goes
  `via` (database / mail) and how to render itself for each one (`server/src/notifications.rs`).

## Local development

Requirements: Rust (stable), Node 22, Postgres 14+.

```sh
createdb timeguessr
cp .env.example .env            # adjust DATABASE_URL
# backend (http://localhost:8080), migrations run at startup
cd server && set -a && . ../.env && set +a && cargo run
# frontend with hot reload (http://localhost:5173, proxies /api to :8080)
cd web && npm install && npm run dev
```

With `MAIL_DRIVER=log`, emails are printed to the backend logs and stored in the `emails` table.

## Tests

```sh
cd server && cargo test                      # unit tests (scoring, scheduling, parsing, images...)
cd web && npm run build                      # typecheck + build
cd e2e && npm install && npx playwright test # boots the backend on a fresh `timeguessr_e2e` database
```

E2E against a deployed instance (emails are read back via the Resend API, using `@resend.dev` test inboxes):

```sh
E2E_REMOTE=1 E2E_BASE_URL=https://timeguessr.procrastination.land RESEND_API_KEY=... npx playwright test journey
```

## Configuration

| Variable | Default | |
|---|---|---|
| `DATABASE_URL` | required | Postgres connection string |
| `APP_URL` | `http://localhost:8080` | Public URL used in emails |
| `MAIL_DRIVER` | `log` | `resend` or `log` |
| `RESEND_API_KEY` | | Required with `MAIL_DRIVER=resend` (Railway secret, never committed) |
| `MAIL_FROM` | `TimeGuessr <noreply@timeguessr.procrastination.land>` | Must be on a verified Resend domain |
| `ORGANIZER_EMAILS` | empty (anyone) | Optional allowlist: `a@x.com, @company.com` |
| `MAX_PARTICIPANTS_PER_GAME` | 200 | |
| `MAX_EMAILS_PER_ORGANIZER_PER_DAY` | 400 | Invitations, reminders and play links |

## Deployment (Railway)

The `Dockerfile` builds the frontend and backend into one small image (`railway.json` sets the health check).
A Railway Postgres database provides `DATABASE_URL`. Secrets live only in Railway variables.
