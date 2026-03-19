#!/bin/bash
set -e

# Railway sets PORT env var — update paygate config to use it
if [ -n "$PORT" ]; then
  sed -i "s/listen = \"0.0.0.0:8080\"/listen = \"0.0.0.0:$PORT\"/" /app/paygate.toml
fi

# Start demo server in background
node /app/dist/server.js &
DEMO_PID=$!

# Wait for demo server to be ready
for i in $(seq 1 10); do
  curl -sf http://localhost:3001/v1/pricing > /dev/null 2>&1 && break
  sleep 1
done

# Start PayGate (foreground)
exec paygate serve --config /app/paygate.toml
