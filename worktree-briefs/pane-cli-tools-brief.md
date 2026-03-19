# Brief: CLI Tools — create-paygate + paygate register

## Objective

Two deliverables in one pane:
1. `create-paygate` — npm package that scaffolds a new PayGate-wrapped API project
2. `paygate register` — Rust CLI subcommand that registers a service in the on-chain PayGateRegistry

---

## Part 1: create-paygate

### Directory Structure

```
packages/create-paygate/
  package.json
  tsconfig.json
  src/
    index.ts           # CLI entrypoint (#!/usr/bin/env node)
    wizard.ts          # Interactive prompts
    scaffold.ts        # File generation
    templates/
      paygate.toml.ts  # Template function
      server.js.ts     # Template function
      dockerfile.ts    # Template function
      readme.md.ts     # Template function
      env.example.ts   # Template function
  tests/
    scaffold.test.ts
    wizard.test.ts
  dist/                # (gitignored, built output)
```

### package.json

```json
{
  "name": "create-paygate",
  "version": "0.1.0",
  "description": "Scaffold a PayGate-wrapped API in 60 seconds",
  "bin": {
    "create-paygate": "dist/index.js"
  },
  "files": ["dist"],
  "scripts": {
    "build": "tsc",
    "test": "vitest run",
    "prepublishOnly": "npm run build"
  },
  "dependencies": {
    "prompts": "^2.4"
  },
  "devDependencies": {
    "typescript": "^5.4",
    "vitest": "^1.3",
    "@types/prompts": "^2.4",
    "@types/node": "^20"
  },
  "keywords": ["paygate", "api", "micropayments", "tempo", "blockchain"],
  "license": "MIT"
}
```

### CLI Flow (src/index.ts)

```
$ npx create-paygate my-api

  create-paygate v0.1.0
  ---------------------

  What does your API do? > Image classification API
  Price per request in USDC? [0.001] > 0.005
  Your Tempo wallet address? > 0x7F3a...
    (or press enter to generate a new one)

  Creating my-api/...
    paygate.toml     config
    server.js        sample API server
    Dockerfile       ready for fly.io
    README.md        quickstart guide
    .env.example     environment template

  Done! Next steps:

    cd my-api
    cp .env.example .env
    # Edit .env with your PAYGATE_PRIVATE_KEY
    npm install
    npm start

  Deploy to fly.io:
    fly launch
    fly secrets set PAYGATE_PRIVATE_KEY=<your-key>
    fly deploy
```

### Wizard (src/wizard.ts)

Use the `prompts` npm package for interactive input:

1. **Target directory**: from CLI arg (`npx create-paygate my-api`) or prompt if missing
   - Validate: no existing directory with that name (unless `--force`)
   - Create the directory

2. **Description**: "What does your API do?"
   - Free text, required, max 200 chars
   - Used in `paygate.toml` `provider.description` and `README.md`

3. **Price**: "Price per request in USDC?"
   - Default: `0.001`
   - Validate: positive number, max 6 decimal places
   - Parse and display as both decimal and "that's X cents per request"

4. **Wallet address**: "Your Tempo wallet address?"
   - Validate: starts with `0x`, exactly 42 chars, valid hex
   - If user presses enter (empty): offer to generate one
   - Generation: use `crypto.randomBytes(32)` to create a private key, derive address using viem's `privateKeyToAccount` (add `viem` as optional dep) or manual secp256k1
   - If generating: print the private key ONCE with a warning to save it
   - **Alternative (simpler)**: skip generation, just require the address. Print a hint: "Generate one with: `cast wallet new` or use MetaMask"

### Templates (src/templates/)

Each template is a function that takes wizard answers and returns a string.

#### paygate.toml.ts

```toml
[gateway]
listen = "0.0.0.0:8080"
upstream = "http://localhost:3000"

[tempo]
network = "testnet"
rpc_urls = ["https://rpc.moderato.tempo.xyz"]
private_key_env = "PAYGATE_PRIVATE_KEY"
accepted_token = "0x20c0000000000000000000000000000000000000"

[provider]
address = "${walletAddress}"
name = "${directoryName}"
description = "${description}"

[sponsorship]
enabled = true
budget_per_day = "1.00"
max_per_tx = "0.01"

[pricing]
default_price = "${price}"

[pricing.endpoints]
"GET /v1/pricing" = "0.000"
"POST /v1/echo" = "${price}"
```

#### server.js.ts

A simple Express echo server:

```javascript
const express = require('express');
const app = express();
app.use(express.json());

// Free pricing endpoint
app.get('/v1/pricing', (req, res) => {
  res.json({
    apis: [{
      endpoint: 'POST /v1/echo',
      price: '${price}',
      currency: 'USDC',
      description: '${description}'
    }]
  });
});

// Your API endpoint
app.post('/v1/echo', (req, res) => {
  // TODO: Replace with your actual API logic
  res.json({
    message: 'Hello from ${name}!',
    input: req.body,
    timestamp: new Date().toISOString()
  });
});

const PORT = process.env.PORT || 3000;
app.listen(PORT, () => {
  console.log('API server listening on port ' + PORT);
});
```

#### dockerfile.ts

```dockerfile
# Multi-stage: PayGate binary + your API server
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
```

#### readme.md.ts

```markdown
# ${name}

${description}

Powered by [PayGate](https://github.com/ssreeni1/paygate) — per-request stablecoin payments on Tempo.

## Quick Start

\`\`\`bash
cp .env.example .env
# Edit .env with your PAYGATE_PRIVATE_KEY
npm install
npm start
\`\`\`

## Test It

\`\`\`bash
# Get pricing (free)
curl http://localhost:8080/v1/pricing

# Try the API (will return 402 Payment Required)
curl -X POST http://localhost:8080/v1/echo -H "Content-Type: application/json" -d '{"hello": "world"}'
\`\`\`

## Deploy to fly.io

\`\`\`bash
fly launch
fly secrets set PAYGATE_PRIVATE_KEY=<your-tempo-private-key>
fly deploy
\`\`\`

Your API is now live and accepting per-request payments!

## How It Works

1. Client sends a request to your API
2. PayGate returns 402 with pricing info
3. Client pays on-chain (USDC on Tempo)
4. Client retries with payment proof
5. PayGate verifies and forwards to your API
6. Client gets the response

Learn more: [PayGate Documentation](https://ssreeni1.github.io/paygate)
```

#### env.example.ts

```
# Your Tempo private key (never commit this!)
PAYGATE_PRIVATE_KEY=

# Port for your API server (PayGate proxies to this)
PORT=3000
```

### Tests

#### scaffold.test.ts

1. Run scaffold with test inputs into a temp directory
2. Verify all 5 files exist
3. Verify `paygate.toml` contains the wallet address and price from inputs
4. Verify `paygate.toml` is valid TOML (parse it)
5. Verify `server.js` is valid JavaScript (try to parse with `new Function` or just check key strings)
6. Verify `Dockerfile` contains `EXPOSE 8080`
7. Verify `.env.example` contains `PAYGATE_PRIVATE_KEY`

#### wizard.test.ts

1. Test address validation: valid address passes, invalid rejected
2. Test price validation: "0.001" passes, "-1" rejected, "abc" rejected
3. Test directory name: "my-api" passes, "" rejected

---

## Part 2: paygate register

### Changes to main.rs

Add a `Register` variant to the `Commands` enum:

```rust
/// Register service in on-chain PayGateRegistry
Register {
    /// Service name
    #[arg(long)]
    name: String,

    /// Price per request in USDC (e.g., "0.001")
    #[arg(long)]
    price: String,

    /// Accepted TIP-20 token address
    #[arg(long, default_value = "0x20c0000000000000000000000000000000000000")]
    token: String,

    /// URL to pricing/metadata JSON
    #[arg(long, default_value = "")]
    metadata_url: String,

    /// PayGateRegistry contract address
    #[arg(long, default_value = "")]
    registry: String,

    /// Config file path
    #[arg(short, long, default_value = "paygate.toml")]
    config: String,
},
```

### Implementation: cmd_register

```rust
async fn cmd_register(name: &str, price: &str, token: &str, metadata_url: &str, registry: &str, config_path: &str) {
    let config = load_config_or_exit(config_path);

    // 1. Parse price to base units
    let price_base_units = parse_price_to_base_units(price)
        .unwrap_or_else(|e| { eprintln!("error: invalid price: {e}"); std::process::exit(1); });

    // 2. Load private key from env
    let private_key = std::env::var(&config.tempo.private_key_env)
        .unwrap_or_else(|_| {
            eprintln!("error: {} not set", config.tempo.private_key_env);
            eprintln!("  hint: export {}=<your-tempo-private-key>", config.tempo.private_key_env);
            std::process::exit(1);
        });

    // 3. Create signer
    let signer: PrivateKeySigner = private_key.parse()
        .unwrap_or_else(|_| { eprintln!("error: invalid private key"); std::process::exit(1); });
    let provider_address = signer.address();

    // 4. Determine registry address
    //    - From --registry flag
    //    - Or from a known deployed address (hardcode testnet address)
    //    - Or fail with "no registry address"

    // 5. Encode the registerService call
    //    Use alloy's sol! macro or manual ABI encoding
    //    registerService(string name, uint256 pricePerRequest, address acceptedToken, string metadataUri)
    //    Function selector: keccak256("registerService(string,uint256,address,string)")[:4]

    // 6. Build and sign the transaction
    //    - to: registry contract address
    //    - data: encoded call
    //    - chain_id: from config
    //    - nonce: fetch from RPC (eth_getTransactionCount)
    //    - gas: estimate or use fixed 200_000

    // 7. Send via eth_sendRawTransaction to Tempo RPC

    // 8. Wait for receipt

    // 9. Decode the serviceId from the receipt logs (ServiceRegistered event)

    // 10. Print results
}
```

### Output Format

```
  PayGate Registry
  ----------------
  Registering service...

  Name:     my-api
  Price:    $0.001/request
  Token:    0x20c0...0000
  Provider: 0x7F3a...0001

  Transaction: 0xabc123...def456
  Service ID:  0x789...012

  View on explorer:
    https://explore.moderato.tempo.xyz/tx/0xabc123...def456

  Your service is now discoverable on-chain!
```

### Contract ABI

Read from `contracts/src/PayGateRegistry.sol`. The key function:

```solidity
function registerService(
    string calldata name,
    uint256 pricePerRequest,
    address acceptedToken,
    string calldata metadataUri
) external returns (bytes32 serviceId)
```

Event to decode from receipt:
```solidity
event ServiceRegistered(bytes32 indexed serviceId, address indexed provider, uint256 price)
```

Use alloy's `sol!` macro to generate the ABI bindings:

```rust
use alloy::sol;

sol! {
    function registerService(
        string name,
        uint256 pricePerRequest,
        address acceptedToken,
        string metadataUri
    ) external returns (bytes32 serviceId);

    event ServiceRegistered(bytes32 indexed serviceId, address indexed provider, uint256 price);
}
```

### Dependencies

Check which alloy crates are already in `Cargo.toml`. May need:
- `alloy-primitives` (Address, U256, etc.)
- `alloy-sol-types` (sol! macro, encoding)
- `alloy-signer-local` (PrivateKeySigner)
- `alloy-consensus` (transaction types)
- `alloy-network` (for signing transactions)

Or use raw ABI encoding with the existing HTTP client to make JSON-RPC calls manually (simpler, fewer deps):
- Encode calldata manually: selector + ABI-encoded params
- Send via `eth_sendRawTransaction`
- This approach avoids pulling in the full alloy provider stack

### Explorer URLs

- Testnet: `https://explore.moderato.tempo.xyz/tx/{hash}`
- Mainnet: `https://explore.tempo.xyz/tx/{hash}`

Select based on `config.tempo.network`.

### Error Cases

| Error | Message |
|-------|---------|
| No private key | `error: PAYGATE_PRIVATE_KEY not set` + hint |
| Invalid price | `error: invalid price "abc" — must be a decimal number` |
| RPC unreachable | `error: Tempo RPC unreachable` + hint |
| Transaction reverted | `error: registration transaction reverted` + tx hash for debugging |
| No registry address | `error: no registry contract address — pass --registry or deploy with paygate contract deploy` |
| Insufficient gas | `error: insufficient balance for gas — fund your wallet first` |

### Tests

For `paygate register`, add a unit test that:
1. Verifies the ABI encoding of `registerService` produces the correct calldata (compare against a known-good encoding)
2. Verifies `ServiceRegistered` event topic matches expected keccak256

The actual on-chain registration is an integration test that requires a testnet -- add it to the E2E test suite (`sdk/testnet-e2e.mjs`) as a new optional step.

## Key Constraints

- create-paygate must work with `npx` (no global install required)
- create-paygate has zero runtime dependencies beyond `prompts` — keep it minimal
- The Rust CLI register command reuses existing config loading and RPC infrastructure
- Private keys are NEVER logged or written to files
- Testnet PathUSD token address: `0x20c0000000000000000000000000000000000000`
