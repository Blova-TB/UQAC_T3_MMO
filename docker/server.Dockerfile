# --- Stage 1: Builder ---
FROM rust:latest AS builder
WORKDIR /app

RUN apt-get update && apt-get install -y pkg-config libudev-dev && rm -rf /var/lib/apt/lists/*

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

# On copie uniquement le serveur et le code partagé
COPY crates/shared ./crates/shared
COPY crates/server ./crates/server

RUN touch crates/server/src/main.rs && \
    cargo build --release -p server

# --- Stage 2: Runtime ---
FROM debian:bookworm-slim
WORKDIR /app

RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/server /usr/local/bin/server

EXPOSE 4000/udp

CMD ["server"]