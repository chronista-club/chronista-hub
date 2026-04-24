# syntax=docker/dockerfile:1.4
# =============================================================================
# Chronista Hub Server — 最小 multi-stage Bun build
# =============================================================================

FROM oven/bun:1 AS deps

WORKDIR /app

# ワークスペース解決のため root package.json + lockfile を copy
COPY package.json ./
COPY bun.lock* ./
COPY tsconfig.json ./
COPY apps/chronista-hub-server/package.json apps/chronista-hub-server/package.json

RUN bun install

# =============================================================================
FROM oven/bun:1-slim AS production

WORKDIR /app

ENV NODE_ENV=production
ENV CHRONISTA_HUB_PORT=3000

COPY --from=deps /app/node_modules ./node_modules
COPY --from=deps /app/package.json ./
COPY --from=deps /app/apps/chronista-hub-server/node_modules apps/chronista-hub-server/node_modules
COPY apps/chronista-hub-server/ apps/chronista-hub-server/

EXPOSE 3000

HEALTHCHECK --interval=30s --timeout=3s --start-period=10s --retries=3 \
  CMD bun -e "fetch('http://127.0.0.1:3000/health').then(r => process.exit(r.ok ? 0 : 1))"

WORKDIR /app/apps/chronista-hub-server

CMD ["bun", "run", "src/index.ts"]
