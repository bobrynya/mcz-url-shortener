# ---------- build stage ----------
FROM rust:1.75-bookworm AS builder
WORKDIR /app

# Кешируем зависимости
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY migrations ./migrations

# Если используешь sqlx::query! (compile-time проверка), то:
# - Либо собирай с подготовленным кэшем (.sqlx) + SQLX_OFFLINE=true
# - Либо обеспечь DATABASE_URL во время сборки (сложнее в docker build)
# Здесь пойдём по простому пути: build без оффлайн-кэша НЕ гарантирован.
RUN cargo build --release

# ---------- runtime stage ----------
FROM debian:bookworm-slim
WORKDIR /app

# (опционально) ca-certificates нужны для исходящих https-запросов, и иногда для TLS-драйверов
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/url-shortener /app/url-shortener
COPY migrations /app/migrations

ENV LISTEN=0.0.0.0:3000
EXPOSE 3000

CMD ["/app/url-shortener"]
