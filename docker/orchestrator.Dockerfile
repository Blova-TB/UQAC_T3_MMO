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

COPY --from=planner /app/recipe.json recipe.json
# On ne compile QUE l'arbre de dépendances de l'orchestrator
RUN cargo chef cook --release --recipe-path recipe.json -p orchestrator

COPY . .
RUN cargo build --release -p orchestrator

# --- Stage 3: Runtime ---
FROM debian:bookworm-slim
WORKDIR /app

# IMPORTANT : L'orchestrateur a besoin de docker.io pour lancer les serveurs de jeu
RUN apt-get update && \
    apt-get install -y ca-certificates docker.io && \
    rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/orchestrator /usr/local/bin/orchestrator

EXPOSE 8080/tcp
EXPOSE 4000/tcp

CMD ["orchestrator"]