import type { WizardAnswers } from '../wizard.js';

export function paygateToml(answers: WizardAnswers): string {
  return `[gateway]
listen = "0.0.0.0:8080"
upstream = "http://localhost:3000"

[tempo]
network = "testnet"
rpc_urls = ["https://rpc.moderato.tempo.xyz"]
private_key_env = "PAYGATE_PRIVATE_KEY"
accepted_token = "0x20c0000000000000000000000000000000000000"

[provider]
address = "${answers.walletAddress}"
name = "${answers.directory}"
description = "${answers.description}"

[sponsorship]
enabled = true
budget_per_day = "1.00"
max_per_tx = "0.01"

[pricing]
default_price = "${answers.price}"

[pricing.endpoints]
"GET /v1/pricing" = "0.000"
"POST /v1/echo" = "${answers.price}"
`;
}
