FROM rust:1.88-slim-bookworm AS builder

WORKDIR /src

COPY Cargo.toml Cargo.lock* ./
COPY build.rs ./
COPY proto ./proto/
RUN mkdir -p src \
 && echo 'fn main() {}' > src/main.rs \
 && cargo build --release --locked 2>/dev/null || cargo build --release \
 && rm -rf src

COPY src ./src/
RUN touch src/main.rs && cargo build --release

FROM debian:bookworm-slim

RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl \
 && rm -rf /var/lib/apt/lists/* \
 && apt-get clean

WORKDIR /app

COPY --from=builder /src/target/release/engine /app/engine
COPY static /app/static/

ENV RUST_LOG=engine=info \
    SYNTRA_BIND_ADDR=0.0.0.0:3002 \
    SYNTRA_VAULT_PATH=/data/essence_vault \
    SYNTRA_SLED_INDEX=/data/metadata_index.sled \
    SYNTRA_DICT_PATH=/data/dictionaries \
    SYNTRA_WATCH_DIR=/data/watch_in \
    SYNTRA_DB_URL=postgres://syntra:syntra123@postgres:5432/syntra_monitoring \
    SYNTRA_DEFAULT_EFFORT=balanced \
    SYNTRA_VERIFY_ON_WRITE=true

RUN useradd --create-home --uid 1000 syntra \
 && mkdir -p /data/essence_vault /data/watch_in /data/dictionaries \
 && chown -R syntra:syntra /app /data
USER syntra
VOLUME ["/data"]

EXPOSE 3002

HEALTHCHECK --interval=30s --timeout=3s --start-period=10s --retries=3 \
    CMD curl -fsS http://127.0.0.1:3002/ready || exit 1

CMD ["./engine"]
