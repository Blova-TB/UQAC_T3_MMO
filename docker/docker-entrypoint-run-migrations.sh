#!/bin/sh
set -e

# Wrapper entrypoint: if invoked to run Postgres, start the original entrypoint
# in background, wait for readiness, run migrations from /migrations, then wait.

ORIG_ENTRYPOINT=/usr/local/bin/docker-entrypoint.sh

if [ "${1:-}" = "postgres" ] || [ $# -eq 0 ]; then
  # Start postgres in background using original entrypoint
  "$ORIG_ENTRYPOINT" postgres &
  PG_PID=$!

  # wait for postgres to become available
  ATTEMPTS=0
  until pg_isready -h localhost -p "${PGPORT:-5432}" -U "${POSTGRES_USER:-postgres}" >/dev/null 2>&1; do
    ATTEMPTS=$((ATTEMPTS+1))
    if [ $ATTEMPTS -ge 30 ]; then
      echo "Postgres did not become ready in time" >&2
      kill $PG_PID || true
      wait $PG_PID || true
      exit 1
    fi
    sleep 1
  done

  # Run SQL migrations if folder exists
  if [ -d /migrations ]; then
    for f in /migrations/*.sql; do
      [ -e "$f" ] || continue
      echo "==> Executing $f"
      PGPASSWORD="${POSTGRES_PASSWORD}" psql -v ON_ERROR_STOP=1 -h localhost -p "${PGPORT:-5432}" -U "${POSTGRES_USER:-postgres}" -d "${POSTGRES_DB:-${POSTGRES_USER:-postgres}}" -f "$f"
    done
  fi

  # Wait for the Postgres process
  wait $PG_PID
else
  exec "$@"
fi

