FROM rust:1-bookworm AS rust-builder
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY lib/crates lib/crates
COPY openapi openapi
RUN cargo build --release --bin arc

FROM oven/bun:1
RUN apt-get update && apt-get install -y --no-install-recommends \
    python3 make g++ ca-certificates nodejs npm && \
    rm -rf /var/lib/apt/lists/*
RUN npm i -g mintlify && \
    sed -i 's/const version = __VERSION__/const version = "0.0.0"/' \
      /usr/local/lib/node_modules/mintlify/node_modules/katex/dist/katex.mjs

WORKDIR /app

COPY --from=rust-builder /app/target/release/arc /usr/local/bin/arc

COPY package.json bun.lock ./
COPY apps/fabro-web/package.json apps/fabro-web/
COPY lib/packages/fabro-api-client/package.json lib/packages/fabro-api-client/
RUN bun install --frozen-lockfile

COPY apps/fabro-web apps/fabro-web
COPY lib/packages/fabro-api-client lib/packages/fabro-api-client
COPY docs docs
COPY docker/entrypoint.ts .

ENTRYPOINT ["bun", "run", "entrypoint.ts"]
