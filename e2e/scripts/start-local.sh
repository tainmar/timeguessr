#!/bin/sh
# Starts the backend against a fresh E2E database (used by Playwright's webServer).
set -e
DB_URL=${E2E_DATABASE_URL:-postgres://postgres@localhost:5433/timeguessr_e2e}
psql "$DB_URL" -q -c "DROP SCHEMA public CASCADE; CREATE SCHEMA public;" >/dev/null
cd "$(dirname "$0")/../../server"
export DATABASE_URL="$DB_URL"
export APP_URL=${E2E_BASE_URL:-http://localhost:8090}
export PORT=${E2E_PORT:-8090}
export MAIL_DRIVER=log
export OUTBOX_INTERVAL_MS=300
export SCHEDULER_INTERVAL_SECS=1
export STATIC_DIR="$(pwd)/../web/dist"
export RUST_LOG=${RUST_LOG:-info}
exec ./target/${E2E_PROFILE:-debug}/timeguessr
