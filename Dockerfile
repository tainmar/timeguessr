# syntax=docker/dockerfile:1

# ---- Frontend ----
FROM node:22-bookworm-slim AS web
WORKDIR /app/web
COPY web/package.json web/package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY web/ ./
RUN npm run build

# ---- Backend ----
FROM rust:1-bookworm AS server
WORKDIR /app/server
# Build dependencies first so they are cached between code changes.
COPY server/Cargo.toml server/Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs \
    && cargo build --release --locked \
    && rm -rf src target/release/deps/timeguessr* target/release/timeguessr*
COPY server/ ./
RUN cargo build --release --locked

# ---- Runtime ----
FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 app
WORKDIR /app
COPY --from=server /app/server/target/release/timeguessr /app/timeguessr
COPY --from=web /app/web/dist /app/web/dist
ENV STATIC_DIR=/app/web/dist \
    PORT=8080 \
    RUST_LOG=info
USER app
EXPOSE 8080
CMD ["/app/timeguessr"]
