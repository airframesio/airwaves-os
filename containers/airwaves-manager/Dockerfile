FROM rust:1-bookworm AS builder

WORKDIR /build

# Cache dependencies by building with dummy src first
COPY Cargo.toml Cargo.lock* ./
RUN mkdir src && echo 'fn main() { println!("placeholder"); }' > src/main.rs
RUN cargo build --release 2>/dev/null || true
RUN rm -rf src

# Build the actual project
COPY src/ src/
RUN touch src/main.rs && cargo build --release

# Runtime image
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    curl \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/airwaves-manager /usr/local/bin/

LABEL org.opencontainers.image.source="https://github.com/airframesio/airwaves-os"
LABEL org.opencontainers.image.description="Airwaves OS Manager - System management API"

EXPOSE 8080

HEALTHCHECK --interval=30s --timeout=5s --retries=3 \
    CMD curl -f http://localhost:8080/health || exit 1

ENTRYPOINT ["airwaves-manager"]
