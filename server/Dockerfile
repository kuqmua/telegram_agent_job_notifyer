FROM rustlang/rust:nightly-bookworm AS builder
WORKDIR /workspace
COPY Cargo.toml Cargo.lock ./
COPY server/Cargo.toml server/Cargo.toml
COPY server/build.rs server/build.rs
COPY server/src server/src
COPY server/tests server/tests
RUN cargo build --release -p server

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates bubblewrap tini curl bash \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /workspace/target/release/server /usr/local/bin/telegram-agent-server
ENTRYPOINT ["/usr/bin/tini", "--", "/usr/local/bin/telegram-agent-server"]
