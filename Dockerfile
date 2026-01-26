# ---------- build stage ----------
FROM rust:1.93-bookworm AS builder
WORKDIR /app

# Кешируем зависимости
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release
RUN rm -rf src

# Копируем реальный код
COPY src ./src
COPY migrations ./migrations

# Пересобираем с реальным кодом
RUN touch src/main.rs && cargo build --release

# ---------- runtime stage ----------
FROM debian:bookworm-slim
WORKDIR /app

# Устанавливаем зависимости для runtime
RUN apt-get update && \
    apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/url-shortener /app/url-shortener
COPY migrations /app/migrations

ENV LISTEN=0.0.0.0:3000
EXPOSE 3000

CMD ["/app/url-shortener"]
