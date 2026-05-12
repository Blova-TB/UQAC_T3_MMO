# --- Stage 1: Builder ---
FROM rust:1.77-bookworm AS builder
WORKDIR /app

# Dépendances système minimales
RUN apt-get update && apt-get install -y pkg-config libudev-dev && rm -rf /var/lib/apt/lists/*

# Optimisation du cache : Manifests uniquement
COPY Cargo.toml Cargo.lock ./
COPY crates/shared/Cargo.toml crates/shared/
COPY crates/server/Cargo.toml crates/server/
COPY crates/client/Cargo.toml crates/client/
COPY crates/orchestrator/Cargo.toml crates/orchestrator/

# Création de fichiers sources factices pour builder les dépendances (cache)
RUN mkdir -p crates/shared/src crates/server/src crates/client/src crates/orchestrator/src && \
    touch crates/shared/src/lib.rs && \
    echo "fn main() {}" > crates/server/src/main.rs && \
    echo "fn main() {}" > crates/client/src/main.rs && \
    echo "fn main() {}" > crates/orchestrator/src/main.rs

RUN cargo build --release -p server

# Copie du code source réel
COPY crates ./crates

# Invalidation du cache des fichiers sources et build final du binaire ciblé
RUN touch crates/server/src/main.rs
RUN cargo build --release -p server

# --- Stage 2: Runtime ---
FROM debian:bookworm-slim
WORKDIR /app

# Certificats pour quinn (si communication sortante nécessaire)
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

# Extraction du binaire ciblé depuis le dossier target du workspace
COPY --from=builder /app/target/release/server /app/server

# Exposition du port QUIC (doit correspondre à la config game_sockets)
EXPOSE 4000/udp

# Utilisation de la forme exec (tableau) pour propager correctement les signaux OS (SIGTERM)
CMD ["./server"]