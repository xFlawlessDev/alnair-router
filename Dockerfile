# syntax=docker/dockerfile:1

# --- Dashboard -------------------------------------------------------------
FROM node:22-alpine AS web
RUN npm install -g pnpm@11.17.0
WORKDIR /app/apps/web
COPY apps/web/package.json apps/web/pnpm-lock.yaml apps/web/pnpm-workspace.yaml ./
RUN pnpm install --frozen-lockfile
COPY apps/web/ ./
RUN pnpm run build

# --- Router ----------------------------------------------------------------
FROM rust:1.85-slim-bookworm AS build
RUN apt-get update \
    && apt-get install -y --no-install-recommends build-essential pkg-config \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY --from=web /app/apps/web/dist ./apps/web/dist
RUN cargo build --release --locked -p alnair-router

# --- Runtime ---------------------------------------------------------------
FROM debian:bookworm-slim AS runtime
RUN useradd --system --uid 10001 --create-home alnair \
    && mkdir -p /data \
    && chown -R alnair:alnair /data
COPY --from=build /app/target/release/alnair-router /usr/local/bin/alnair-router

ENV ALNAIR_ROUTER_HOME=/data
USER alnair
VOLUME ["/data"]
EXPOSE 7878

ENTRYPOINT ["alnair-router"]
