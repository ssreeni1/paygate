# Stage 1: Build PayGate binary
FROM rust:1.77-slim AS paygate-build
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/
RUN cargo build --release -p paygate-gateway

# Stage 2: Build demo server
FROM node:20-slim AS demo-build
WORKDIR /app
COPY demo/package.json demo/package-lock.json* ./
RUN npm install
COPY demo/ .
RUN npm run build
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
COPY --from=demo-build /app/dist ./dist
COPY --from=demo-build /app/node_modules ./node_modules
COPY --from=demo-build /root/.cache/ms-playwright /root/.cache/ms-playwright
COPY demo/paygate.toml /app/paygate.toml
COPY demo/entrypoint.sh /app/entrypoint.sh

RUN chmod +x /app/entrypoint.sh
EXPOSE 8080

CMD ["/app/entrypoint.sh"]
