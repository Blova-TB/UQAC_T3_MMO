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
# On compile uniquement le broker
RUN cargo chef cook --release --recipe-path recipe.json -p broker

COPY . .
RUN cargo build --release -p broker

# --- Stage 3: Runtime ---
FROM debian:bookworm-slim
WORKDIR /app

# On ajoute iproute2 pour que la commande `ss` du healthcheck fonctionne
RUN apt-get update && \
    apt-get install -y ca-certificates iproute2 && \
    rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/broker /usr/local/bin/broker

EXPOSE 5000/udp

CMD ["broker"]