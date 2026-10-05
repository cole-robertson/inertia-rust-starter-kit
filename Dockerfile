# syntax=docker/dockerfile:1
# check=error=true

# This Dockerfile is designed for production, not development. Use with Kamal or build'n'run by hand:
# docker build -t inertia_rust_starter_kit .
# docker run -d -p 80:80 -v inertia_rust_starter_kit_storage:/app/storage \
#   -e SECRET_KEY_BASE=<64+ random chars> -e HOST=https://app.example.com \
#   --name inertia_rust_starter_kit inertia_rust_starter_kit
#
# For development, use bin/setup and bin/dev on the host instead.

# Client-side rendering by default. Set to "true" to build and ship the SSR runtime (Node and
# ssr/ssr.js, ~140 MB more) and turn SSR on at run time; flip and rebuild to toggle.
ARG SSR_ENABLED=false

# NODE_VERSION must match .node-version and RUST_VERSION must match rust-toolchain.toml: if the
# toolchain file names another version, `COPY . .` makes rustup switch toolchains mid-build and the
# dependency layer cooked by cargo-chef is thrown away (everything compiles twice).
ARG NODE_VERSION=22.23.2
ARG RUST_VERSION=1.98.1
ARG CARGO_CHEF_VERSION=0.1.78

# -----------------------------------------------------------------------------
# Frontend: Vite client bundle (public/vite) and, optionally, the SSR bundle (ssr/ssr.js)
# -----------------------------------------------------------------------------
FROM docker.io/library/node:${NODE_VERSION}-bookworm-slim AS assets
WORKDIR /app

COPY package.json package-lock.json ./
RUN --mount=type=cache,target=/root/.npm npm ci

COPY vite.config.ts tsconfig.json tsconfig.app.json tsconfig.node.json components.json ./
COPY frontend ./frontend

ARG SSR_ENABLED
RUN npx vite build && \
    if [ "$SSR_ENABLED" = "true" ]; then npx vite build --ssr; else mkdir -p ssr; fi

# -----------------------------------------------------------------------------
# Rust: cargo-chef splits dependency compilation into its own cached layer
# -----------------------------------------------------------------------------
FROM docker.io/lukemathwalker/cargo-chef:${CARGO_CHEF_VERSION}-rust-${RUST_VERSION}-slim-bookworm AS chef
WORKDIR /app

FROM chef AS planner
COPY . .
# Fail fast if the image toolchain and rust-toolchain.toml disagree (see RUST_VERSION above).
ARG RUST_VERSION
RUN grep -q "channel = \"${RUST_VERSION}\"" rust-toolchain.toml || \
    { echo "RUST_VERSION ${RUST_VERSION} != rust-toolchain.toml channel" >&2; exit 1; }
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS build
COPY rust-toolchain.toml ./
COPY --from=planner /app/recipe.json recipe.json
# Dependencies only: this layer is reused until Cargo.toml / Cargo.lock change.
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    cargo chef cook --release --locked --recipe-path recipe.json

COPY . .
# Built assets are in place before compiling, in case anything is embedded at build time.
COPY --from=assets /app/public/vite ./public/vite
# Extra cargo features, e.g. `--build-arg CARGO_FEATURES=bench` for the I/O benchmark image
# (docs/BENCHMARK.md). Empty by default: the shipped image has no benchmark endpoints.
ARG CARGO_FEATURES=""
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    cargo build --release --locked --bin inertia_rust_starter_kit-cli --features "${CARGO_FEATURES}" && \
    cp target/release/inertia_rust_starter_kit-cli /usr/local/bin/

# -----------------------------------------------------------------------------
# Runtime
# -----------------------------------------------------------------------------
FROM docker.io/library/node:${NODE_VERSION}-bookworm-slim AS node-runtime

FROM docker.io/library/debian:bookworm-slim AS base

# tini reaps the supervised `node ssr/ssr.js` child and forwards signals; curl serves the HEALTHCHECK;
# sqlite3 is the database console (`kamal dbc`), as in the Rails image.
RUN apt-get update -qq && \
    apt-get install --no-install-recommends -y ca-certificates curl sqlite3 tini && \
    rm -rf /var/lib/apt/lists /var/cache/apt/archives

WORKDIR /app

ENV LOCO_ENV="production" \
    BINDING="0.0.0.0" \
    PORT="80" \
    DATABASE_URL="sqlite:///app/storage/production.sqlite?mode=rwc" \
    QUEUE_URL="sqlite:///app/storage/queue.sqlite?mode=rwc"

# Branch: SSR enabled — ship the Node runtime alongside the app (the SSR bundle is self-contained,
# vite.config.ts sets ssr.noExternal, so no node_modules are needed).
FROM base AS branch-ssr-true
COPY --from=node-runtime /usr/local/bin/node /usr/local/bin/node
ENV SSR_ENABLED="true"

# Branch: SSR disabled — base only, no JS runtime
FROM base AS branch-ssr-false

# Final stage for app image: picks the right branch by SSR_ENABLED
FROM branch-ssr-${SSR_ENABLED} AS final

# Run and own only the runtime files as a non-root user for security
RUN groupadd --system --gid 1000 app && \
    useradd app --uid 1000 --gid 1000 --create-home --shell /bin/bash && \
    mkdir -p /app/storage && chown app:app /app/storage
USER 1000:1000

COPY --chown=app:app --from=build /usr/local/bin/inertia_rust_starter_kit-cli /app/inertia_rust_starter_kit-cli
COPY --chown=app:app config ./config
COPY --chown=app:app public ./public
COPY --chown=app:app --from=assets /app/public/vite ./public/vite
COPY --chown=app:app --from=assets /app/ssr ./ssr
COPY --chown=app:app bin/docker-entrypoint /app/docker-entrypoint

# SQLite database and queue live here; mount a volume on it.
VOLUME /app/storage

EXPOSE 80
HEALTHCHECK --interval=10s --timeout=3s --start-period=10s --retries=3 \
    CMD curl -fsS "http://127.0.0.1:${PORT}/up" > /dev/null || exit 1

# docker-entrypoint seeds the demo admin when DEMO_ADMIN_EMAIL/DEMO_ADMIN_PASSWORD are set.
ENTRYPOINT ["/usr/bin/tini", "--", "/app/docker-entrypoint"]

# Web server, background queue worker and scheduler in one process; migrations run on boot
# (auto_migrate). With no jobs under `scheduler:` in config the scheduler is not started.
CMD ["/app/inertia_rust_starter_kit-cli", "start", "--all", "--no-banner"]
