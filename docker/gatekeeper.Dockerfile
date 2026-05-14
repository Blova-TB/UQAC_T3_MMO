FROM rust:latest AS builder
WORKDIR /app

ENV SQLX_OFFLINE=true

COPY . .
RUN cargo build --release --bin gatekeeper

FROM debian:bookworm-slim
WORKDIR /app

RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/gatekeeper .

EXPOSE 3000
CMD ["./gatekeeper"]