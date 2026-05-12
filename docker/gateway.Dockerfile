# Stage 1: Build
FROM rust:latest AS builder
WORKDIR /app
# Copie de tout le workspace (nécessaire pour les dépendances locales comme 'shared')
COPY . .
RUN cargo build --release --bin gateway

# Stage 2: Runtime
FROM debian:bookworm-slim
WORKDIR /app
# On ne récupère que le binaire compilé
COPY --from=builder /app/target/release/gateway .
EXPOSE 3000
CMD ["./gateway"]