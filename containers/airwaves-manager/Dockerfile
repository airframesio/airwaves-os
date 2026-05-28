# Stage 1: Generate dependency recipe
FROM rust:1-bookworm AS chef
RUN cargo install cargo-chef
WORKDIR /build

# Stage 2: Plan dependencies (only re-runs if Cargo.toml/lock change)
FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY src/ src/
RUN cargo chef prepare --recipe-path recipe.json

# Stage 3: Build dependencies (cached across code changes)
FROM chef AS deps
COPY --from=planner /build/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json

# Stage 4: Build the application (only recompiles our code)
FROM deps AS builder
COPY Cargo.toml Cargo.lock ./
COPY src/ src/
RUN cargo build --release

# Stage 5: Runtime image
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    curl \
    iproute2 \
    avahi-utils \
    usbutils \
    util-linux \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/airwaves-manager /usr/local/bin/

LABEL org.opencontainers.image.source="https://github.com/airframesio/airwaves-os"
LABEL org.opencontainers.image.description="Airwaves OS Manager - System management API"

EXPOSE 8080

HEALTHCHECK --interval=30s --timeout=5s --retries=3 \
    CMD curl -f http://localhost:8080/health || exit 1

ENTRYPOINT ["airwaves-manager"]
