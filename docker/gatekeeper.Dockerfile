# --- Stage 1: Builder ---
FROM rust:latest AS builder
WORKDIR /app

ENV SQLX_OFFLINE=true

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


# 3. On copie uniquement le vrai code dont on a besoin
COPY crates/gatekeeper ./crates/gatekeeper

# 4. Invalider le cache du fichier principal et compiler le binaire final
RUN touch crates/gatekeeper/src/main.rs && \
    cargo build --release -p gatekeeper

# --- Stage 2: Runtime ---
FROM debian:bookworm-slim
WORKDIR /app

RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

# Placer le binaire dans les exécutables système
COPY --from=builder /app/target/release/gatekeeper /usr/local/bin/gatekeeper

EXPOSE 3000
CMD ["gatekeeper"]