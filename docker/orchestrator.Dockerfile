# --- Stage 1: Builder ---
FROM rust:latest AS builder
WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY crates/shared/Cargo.toml crates/shared/
COPY crates/gatekeeper/Cargo.toml crates/gatekeeper/
COPY crates/server/Cargo.toml crates/server/
COPY crates/client/Cargo.toml crates/client/
COPY crates/orchestrator/Cargo.toml crates/orchestrator/

# 2. Créer des fichiers factices pour TOUT le workspace
RUN mkdir -p crates/shared/src crates/gatekeeper/src crates/server/src crates/client/src crates/orchestrator/src && \
    touch crates/shared/src/lib.rs && \
    echo "fn main() {}" > crates/gatekeeper/src/main.rs && \
    echo "fn main() {}" > crates/server/src/main.rs && \
    echo "fn main() {}" > crates/client/src/main.rs && \
    echo "fn main() {}" > crates/orchestrator/src/main.rs && \
    cargo build --release -p gatekeeper

COPY crates/shared ./crates/shared
COPY crates/orchestrator ./crates/orchestrator

RUN touch crates/orchestrator/src/main.rs && \
    cargo build --release -p orchestrator

# --- Stage 2: Runtime ---
FROM debian:bookworm-slim
WORKDIR /app

# IMPORTANT : On installe 'docker.io' pour que l'orchestrateur puisse builder le serveur
RUN apt-get update && \
    apt-get install -y ca-certificates docker.io && \
    rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/orchestrator /usr/local/bin/orchestrator

EXPOSE 8080/tcp
EXPOSE 4000/tcp

CMD ["orchestrator"]