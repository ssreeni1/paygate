import type { WizardAnswers } from '../wizard.js';

export function readmeMd(answers: WizardAnswers): string {
  return `# ${answers.directory}

${answers.description}

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
`;
}
