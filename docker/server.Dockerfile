# --- Stage 1: Planner (Le Chef) ---
FROM rust:latest AS chef
RUN cargo install cargo-chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# --- Stage 2: Builder (Le Cuisinier) ---
FROM chef AS builder
WORKDIR /app

# IMPORTANT : Le serveur a besoin de ces paquets système pour compiler
RUN apt-get update && \
    apt-get install -y pkg-config libudev-dev && \
    rm -rf /var/lib/apt/lists/*

COPY --from=planner /app/recipe.json recipe.json
# On ne compile QUE l'arbre de dépendances du serveur
RUN cargo chef cook --release --recipe-path recipe.json -p server

COPY . .
RUN cargo build --release -p server

# --- Stage 3: Runtime ---
FROM debian:bookworm-slim
WORKDIR /app

# À l'exécution, le serveur n'a besoin que des certificats de base
RUN apt-get update && \
    apt-get install -y ca-certificates && \
    rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/server /usr/local/bin/server

# Le serveur utilise de l'UDP (souvent le cas pour les MMO / jeux temps réel)
EXPOSE 4000/udp

CMD ["server"]