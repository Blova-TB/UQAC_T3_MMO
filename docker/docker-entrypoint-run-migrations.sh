#!/bin/sh
set -e

# Point d'entrée original de l'image Postgres
ORIG_ENTRYPOINT=/usr/local/bin/docker-entrypoint.sh

if [ "${1:-}" = "postgres" ] || [ $# -eq 0 ]; then
  # 1. Lancement de Postgres en arrière-plan
  "$ORIG_ENTRYPOINT" postgres &
  PG_PID=$!

  # 2. Sécurisation des variables (fallback sur 'postgres' si vide)
  DB_USER="${POSTGRES_USER:-postgres}"
  DB_NAME="${POSTGRES_DB:-$DB_USER}"
  DB_PORT="${PGPORT:-5432}"

  echo "⏳ Attente du démarrage de Postgres (User: $DB_USER, DB: $DB_NAME)..."

  # 3. Attente que la base soit prête à recevoir des requêtes
  ATTEMPTS=0
  until pg_isready -h localhost -p "$DB_PORT" -U "$DB_USER" -d "$DB_NAME" >/dev/null 2>&1; do
    ATTEMPTS=$((ATTEMPTS+1))
    if [ $ATTEMPTS -ge 30 ]; then
      echo "❌ Erreur : Postgres n'est pas prêt après 30 secondes" >&2
      kill $PG_PID || true
      wait $PG_PID || true
      exit 1
    fi
    sleep 1
  done

  echo "✅ Base de données prête !"

  # 4. Exécution des scripts de migration SQL
  if [ -d /migrations ]; then
    for f in /migrations/*.sql; do
      [ -e "$f" ] || continue
      echo "==> Exécution de la migration : $f"
      PGPASSWORD="${POSTGRES_PASSWORD}" psql -v ON_ERROR_STOP=1 -h localhost -p "$DB_PORT" -U "$DB_USER" -d "$DB_NAME" -f "$f"
    done
    echo "✅ Toutes les migrations sont terminées !"
  fi

  # 5. On garde le conteneur en vie en écoutant le processus Postgres
  wait $PG_PID
else
  exec "$@"
fi