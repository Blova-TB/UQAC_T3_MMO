# --- Stage 1: Planner ---
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
# Compilation isolée des dépendances pour le cache
RUN cargo chef cook --release --recipe-path recipe.json -p spatial_server

COPY . .
# Compilation du binaire final
RUN cargo build --release -p spatial_server

# --- Stage 3: Runtime ---
FROM debian:bookworm-slim
WORKDIR /app

# Utilitaires de base requis pour la résolution DNS et TLS (QUIC)
RUN apt-get update && \
    apt-get install -y ca-certificates && \
    rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/spatial_server /usr/local/bin/spatial_server

# Le serveur spatial agit ici principalement comme un client QUIC,
# EXPOSE est facultatif s'il n'écoute pas de port entrant.

CMD ["spatial_server"]