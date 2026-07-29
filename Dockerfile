# ==============================================================================
# BUILDER STAGE - Syntra Engine (Sem Neural/VAE)
# ==============================================================================
FROM rust:1.88-slim-bookworm AS builder

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    cmake \
    clang \
    protobuf-compiler \
    curl \
    libgomp1 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /usr/src/syntra-engine

# 1. Copia arquivos de manifestação para cache de dependências
COPY Cargo.toml ./
COPY proto ./proto/

# 2. Build dependencies (dummy main para cache)
RUN mkdir -p src && echo "fn main() {}" > src/main.rs
RUN cargo build --release && rm -rf src

# 3. Copia código fonte real
COPY . .

# 4. Build final (sem features neurais)
RUN cargo build --release

# ==============================================================================
# PRODUCTION IMAGE
# ==============================================================================
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    libgomp1 \
    libstdc++6 \
    && rm -rf /var/lib/apt/lists/* \
    && apt-get clean

WORKDIR /app

# Copy binary
COPY --from=builder /usr/src/syntra-engine/target/release/engine /app/engine

# Copy static files
COPY --from=builder /usr/src/syntra-engine/static /app/static/

# Expose the API Port
EXPOSE 3002

# Environment Variables
ENV RUST_LOG=engine=debug \
    SYNTRA_VAULT_PATH=/data/essence_vault \
    SYNTRA_SLED_INDEX=/data/metadata_index.sled \
    SYNTRA_WATCH_DIR=/data/watch_in \
    SYNTRA_DB_URL=postgres://syntra:syntra123@postgres:5432/syntra_monitoring \
    PORT=3002

# Non-root user
RUN useradd -m -u 1000 syntra && \
    mkdir -p /data/watch_in /data/essence_vault && \
    chown -R syntra:syntra /app /data

USER syntra

CMD ["./engine"]
