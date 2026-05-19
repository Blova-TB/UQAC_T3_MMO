# --- Stage 1: Planner (Le Chef) ---
# Ce stage analyse le workspace et prépare la "recette" des dépendances
FROM rust:latest AS chef
RUN cargo install cargo-chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# --- Stage 2: Builder ---
FROM chef AS builder
WORKDIR /app

COPY --from=planner /app/recipe.json recipe.json

# On ajoute "-p gatekeeper" pour ne compiler QUE les dépendances de ce crate,
# ignorant ainsi les dépendances graphiques (Wayland) du crate "client".
RUN cargo chef cook --release --recipe-path recipe.json -p gatekeeper

COPY . .

ENV SQLX_OFFLINE=true
RUN cargo build --release -p gatekeeper

# --- Stage 3: Runtime (L'Exécution) ---
FROM debian:bookworm-slim
WORKDIR /app

RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/gatekeeper /usr/local/bin/gatekeeper

EXPOSE 3000
CMD ["gatekeeper"]