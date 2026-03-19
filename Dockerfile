# PayGate Demo Marketplace — Dockerfile for Railway deployment
# Builds Rust gateway + Node.js demo server in a single container

# Stage 1: Build PayGate binary
FROM rust:1.77-slim AS paygate-build
WORKDIR /build
COPY Cargo.toml ./
COPY crates/ crates/
# Create a dummy schema.sql for the build
COPY schema.sql .
RUN cargo build --release -p paygate-gateway

# Stage 2: Build demo server
FROM node:20-slim AS demo-build
WORKDIR /app
COPY demo/package.json ./
RUN npm install
COPY demo/tsconfig.json ./
COPY demo/src/ src/
RUN npx tsc
RUN npx playwright install --with-deps chromium

# Stage 3: Runtime
FROM node:20-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
    curl \
    libnss3 libatk1.0-0 libatk-bridge2.0-0 libcups2 libdrm2 \
    libxkbcommon0 libxcomposite1 libxdamage1 libxrandr2 libgbm1 \
    libpango-1.0-0 libasound2 && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=paygate-build /build/target/release/paygate /usr/local/bin/paygate
COPY --from=paygate-build /build/schema.sql /app/schema.sql
COPY --from=demo-build /app/dist ./dist
COPY --from=demo-build /app/node_modules ./node_modules
COPY --from=demo-build /root/.cache/ms-playwright /root/.cache/ms-playwright
COPY demo/paygate.toml /app/paygate.toml
COPY demo/entrypoint.sh /app/entrypoint.sh

RUN chmod +x /app/entrypoint.sh

# Railway sets PORT env var
ENV PORT=8080
EXPOSE 8080

CMD ["/app/entrypoint.sh"]
