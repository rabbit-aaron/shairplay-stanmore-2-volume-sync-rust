FROM rust:1.87-bookworm AS builder

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
# Cache dependency compilation.
RUN mkdir src && echo "fn main() {}" > src/main.rs && cargo build --release && rm -rf src

COPY src ./src
RUN touch src/main.rs && cargo build --release

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    dumb-init ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/volume-sync /usr/local/bin/volume-sync

ENTRYPOINT ["dumb-init", "--"]
CMD ["volume-sync"]
