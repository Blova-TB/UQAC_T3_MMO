# ==========================================
# ÉTAPE 1 : Compilation (Builder)
# ==========================================
# On demande le compilateur Rust le plus récent (latest), fixé sur l'OS Bookworm
FROM rust:bookworm AS builder

WORKDIR /usr/src/mmo

# Tes dépendances système pour compiler Bevy
RUN apt-get update && apt-get install -y \
    pkg-config \
    libwayland-dev \
    libx11-dev \
    libasound2-dev \
    libudev-dev \
    && rm -rf /var/lib/apt/lists/*

COPY . .

RUN cargo build --release --bin client

# ==========================================
# ÉTAPE 2 : Exécution (Runtime léger)
# ==========================================
FROM debian:bookworm-slim

WORKDIR /app

RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

COPY --from=builder /usr/src/mmo/target/release/client /usr/local/bin/client

CMD ["client"]