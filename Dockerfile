# context-snipe — container image for Glama and any MCP host that runs servers
# in a sandbox. Builds the binary from source, then ships it in a minimal
# runtime image. The server speaks MCP over stdio via `serve`.

# ── Build stage ──────────────────────────────────────────────
FROM rust:slim AS build
WORKDIR /app
COPY . .
RUN cargo build --release && strip target/release/context-snipe

# ── Runtime stage ────────────────────────────────────────────
FROM debian:bookworm-slim
# CA roots so OSV.dev TLS works at runtime (introspection itself needs no network).
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
RUN useradd --create-home --uid 10001 app
COPY --from=build /app/target/release/context-snipe /usr/local/bin/context-snipe
USER app
# Default mode is `serve` (MCP over stdio), so a bare run starts the server.
ENTRYPOINT ["context-snipe"]
CMD ["serve"]
