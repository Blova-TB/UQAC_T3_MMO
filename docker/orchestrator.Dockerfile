# --- Stage 1: Builder ---
FROM rust:1.77-bookworm AS builder
WORKDIR /app

# L'orchestrateur n'a généralement pas besoin de libudev-dev (lié à Bevy)
RUN apt-get update && apt-get install -y pkg-config && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY crates/shared/Cargo.toml crates/shared/
COPY crates/server/Cargo.toml crates/server/
COPY crates/client/Cargo.toml crates/client/
COPY crates/orchestrator/Cargo.toml crates/orchestrator/

RUN mkdir -p crates/shared/src crates/server/src crates/client/src crates/orchestrator/src && \
    touch crates/shared/src/lib.rs && \
    echo "fn main() {}" > crates/server/src/main.rs && \
    echo "fn main() {}" > crates/client/src/main.rs && \
    echo "fn main() {}" > crates/orchestrator/src/main.rs

RUN cargo build --release -p orchestrator

COPY crates ./crates
RUN touch crates/orchestrator/src/main.rs
RUN cargo build --release -p orchestrator

# --- Stage 2: Runtime ---
FROM debian:bookworm-slim
WORKDIR /app

RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/orchestrator /app/orchestrator

# Exposition de l'API HTTP/gRPC de l'orchestrateur
EXPOSE 8080/tcp

CMD ["./orchestrator"]