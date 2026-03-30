# Agent Tips Quickstart

Send stablecoin micropayments to open-source maintainers directly from your AI agent.

## Prerequisites

- Node.js 18+
- A Tempo testnet wallet with USDC (get testnet tokens from the Tempo faucet)

## Step 1: Install

```bash
npm install @paygate/mcp
```

## Step 2: Configure environment variables

Set these in your shell or `.env` file:

| Variable | Required | Description |
|---|---|---|
| `PAYGATE_GATEWAY_URL` | Yes | Gateway URL (e.g. `https://gateway.paygate.dev`) |
| `PAYGATE_PRIVATE_KEY` | Yes* | Hex-encoded private key for your agent wallet |
| `PAYGATE_PRIVATE_KEY_CMD` | Yes* | Alternative: shell command that prints the key (e.g. `op read "op://vault/paygate/key"`) |
| `PAYGATE_AGENT_NAME` | No | Name shown on tip receipts (default: `mcp-agent`) |
| `PAYGATE_SPEND_LIMIT_DAILY` | No | Daily spend cap in USD (e.g. `5.00`) |

*One of `PAYGATE_PRIVATE_KEY` or `PAYGATE_PRIVATE_KEY_CMD` is required.

## Step 3: Add to Claude Code config

Add the MCP server to your Claude Code configuration (`~/.claude/config.json` or project `.claude/config.json`):

```json
{
  "mcpServers": {
    "paygate": {
      "command": "npx",
      "args": ["@paygate/mcp"],
      "env": {
        "PAYGATE_GATEWAY_URL": "https://gateway.paygate.dev",
        "PAYGATE_PRIVATE_KEY_CMD": "op read 'op://vault/paygate/key'",
        "PAYGATE_AGENT_NAME": "my-agent",
        "PAYGATE_SPEND_LIMIT_DAILY": "5.00"
      }
    }
  }
}
```

## Step 4: First tip

Once configured, your agent can tip open-source packages it uses:

```
Agent: I used the `zod` library to validate your schema.
       Tipping the maintainer $0.10...

[paygate] tip zod - $0.100000 - https://testnet.tempo.xyz/tx/0xabc...
```

The agent calls the `tip_open_source` tool with:
- `target`: npm package name (e.g. `zod`) or GitHub username (e.g. `@colinhacks`)
- `amount`: tip amount in USD (e.g. `0.10`)
- `reason`: why the tip is being sent (shown on the receipt)

Tips >= $1.00 require confirmation via the `tip_confirm` tool. The agent receives a token and must explicitly confirm before the payment executes.

## Step 5: Checking your tips

After a tip is sent, the response includes:

- **Receipt URL**: A public page showing the tip details, amount, and on-chain proof
- **Explorer link**: Direct link to the transaction on the Tempo block explorer
- **Leaderboard**: View top tippers at `https://gateway.paygate.dev/paygate/leaderboard`

Use the `tip_report` tool to get a summary of all tips sent in the current session.

## For developers: claiming tips

If you maintain an open-source package that receives tips:

1. Register your package at `https://gateway.paygate.dev/paygate/claim`
2. Link your npm package name or GitHub username to your Tempo wallet address
3. Unclaimed tips are held in escrow for 90 days

### Badge embed

Add a tip badge to your README:

```markdown
[![Tips](https://gateway.paygate.dev/paygate/badge/YOUR_PACKAGE)](https://gateway.paygate.dev/paygate/tip/YOUR_PACKAGE)
```
