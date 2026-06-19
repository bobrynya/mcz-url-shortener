# ---------- chef stage ----------
FROM rust:1.96-slim-bookworm AS chef
WORKDIR /app
RUN apt-get update && apt-get install -y --no-install-recommends \
    cmake g++ make \
    libssl-dev libsasl2-dev libcurl4-openssl-dev zlib1g-dev \
    && rm -rf /var/lib/apt/lists/*
RUN cargo install cargo-chef --version 0.1.77 --locked

# ---------- planner stage ----------
FROM chef AS planner
# `cargo chef prepare` runs `cargo metadata`, which requires the crate targets to
# exist — so the source tree is needed here, not just the manifests.
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo chef prepare --recipe-path recipe.json

# ---------- builder stage ----------
FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json

# Build only dependencies (cached layer)
RUN cargo chef cook --release --locked --recipe-path recipe.json

# Copy source, manifests, migrations, and sqlx query cache.
# askama.toml points the template dir at src/web/templates/ (copied via `src`);
# it must be present or askama falls back to a non-existent `templates/` dir.
COPY Cargo.toml Cargo.lock askama.toml ./
COPY src ./src
COPY migrations ./migrations
COPY .sqlx ./.sqlx

# Build with offline sqlx mode
ENV SQLX_OFFLINE=true
RUN cargo build --release --locked --bin url-shortener
# Note: binary is already stripped via [profile.release] strip = true in Cargo.toml

# ---------- runtime stage ----------
FROM debian:bookworm-slim AS runtime
WORKDIR /app

RUN groupadd -r appuser && useradd -r -g appuser appuser

# ca-certificates: TLS trust store for rustls
# curl: used by Docker healthcheck
RUN apt-get update && \
    apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Copy binary and static assets in one layer with correct ownership
COPY --from=builder --chown=appuser:appuser /app/target/release/url-shortener /app/url-shortener
COPY --chown=appuser:appuser static /app/static

USER appuser

ENV LISTEN=0.0.0.0:8000
EXPOSE 8000

HEALTHCHECK --interval=30s --timeout=5s --start-period=30s --retries=3 \
    CMD curl -f http://localhost:8000/health || exit 1

CMD ["/app/url-shortener"]
