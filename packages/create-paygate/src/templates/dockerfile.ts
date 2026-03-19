export function dockerfile(): string {
  return `# Multi-stage: PayGate binary + your API server
FROM rust:1.77-slim AS paygate
RUN cargo install paygate-gateway
# OR: download pre-built binary from GitHub releases
# RUN curl -fsSL https://github.com/ssreeni1/paygate/releases/latest/download/paygate-linux-amd64 -o /usr/local/bin/paygate && chmod +x /usr/local/bin/paygate

FROM node:20-slim
WORKDIR /app

# Copy PayGate binary
COPY --from=paygate /usr/local/cargo/bin/paygate /usr/local/bin/paygate

# Install dependencies
COPY package*.json ./
RUN npm install --production

# Copy app files
COPY . .

EXPOSE 8080

# Start both servers
CMD ["sh", "-c", "node server.js & sleep 2 && exec paygate serve"]
`;
}
