#!/bin/bash
set -e

# Start demo server in background
node /app/dist/server.js &
DEMO_PID=$!

# Wait for demo server to be ready
for i in $(seq 1 10); do
  curl -sf http://localhost:3001/v1/pricing && break
  sleep 1
done

# Start PayGate (foreground)
exec paygate serve --config /app/paygate.toml
