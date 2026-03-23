import { createClient, http, publicActions, walletActions } from 'viem';
import { privateKeyToAccount } from 'viem/accounts';
import { requestHash, paymentMemo } from './src/hash.js';

const CONSUMER_KEY = '0x55f1eb7043e21b0c7417de5398a423db0cad1603ff50e2fd16f3ac02a5b5dc79';
const GATEWAY = 'https://paygate-demo-production.up.railway.app';
const RPC_URL = 'https://rpc.moderato.tempo.xyz';
const TOKEN = '0x20c0000000000000000000000000000000000000';

const TIP20_ABI = [
  { name: 'transferWithMemo', type: 'function', stateMutability: 'nonpayable',
    inputs: [{ name: 'to', type: 'address' }, { name: 'amount', type: 'uint256' }, { name: 'memo', type: 'bytes32' }],
    outputs: [{ type: 'bool' }] },
];

const account = privateKeyToAccount(CONSUMER_KEY);
const client = createClient({
  account,
  chain: { id: 42431, name: 'Tempo Moderato', nativeCurrency: { name: 'USD', symbol: 'USD', decimals: 6 }, rpcUrls: { default: { http: [RPC_URL] } } },
  transport: http(RPC_URL),
}).extend(publicActions).extend(walletActions);

const ENDPOINTS = [
  { path: '/v1/search', body: { query: 'AI agents autonomous payments' } },
  { path: '/v1/image', body: { prompt: 'robot paying for API calls with digital coins' } },
  { path: '/v1/summarize', body: { text: 'PayGate is a reverse proxy that gates API access behind per-request stablecoin micropayments on the Tempo blockchain. Agents discover the price via a 402 response, pay on-chain, and retry with proof of payment.' } },
  { path: '/v1/search', body: { query: 'stablecoin micropayments blockchain' } },
  { path: '/v1/scrape', body: { url: 'https://example.com' } },
];

async function payForEndpoint(ep, idx) {
  const method = 'POST';
  const bodyStr = JSON.stringify(ep.body);

  const resp = await fetch(`${GATEWAY}${ep.path}`, {
    method, headers: { 'Content-Type': 'application/json' }, body: bodyStr,
  });
  if (resp.status !== 402) { console.log(`[${idx+1}] ${ep.path} — expected 402, got ${resp.status}`); return; }
  const pricing = await resp.json();
  const quoteId = pricing.pricing.quote_id;
  const amount = BigInt(pricing.pricing.amount_base_units);
  const recipient = pricing.pricing.recipient;

  const rh = requestHash(method, ep.path, bodyStr);
  const memo = paymentMemo(quoteId, rh);

  const txHash = await client.writeContract({
    address: TOKEN, abi: TIP20_ABI, functionName: 'transferWithMemo',
    args: [recipient, amount, memo],
  });
  const receipt = await client.waitForTransactionReceipt({ hash: txHash });

  await new Promise(r => setTimeout(r, 500));
  const resp2 = await fetch(`${GATEWAY}${ep.path}`, {
    method,
    headers: {
      'Content-Type': 'application/json',
      'X-Payment-Tx': txHash,
      'X-Payment-Payer': account.address,
      'X-Payment-Quote-Id': quoteId,
    },
    body: bodyStr,
  });

  console.log(`[${idx+1}/5] ${ep.path} — paid ${Number(amount)/1e6} USDC — ${resp2.status} — tx:${txHash.slice(0,20)}...`);
}

console.log('Sending 5 test payments through the live marketplace...\n');
for (let i = 0; i < ENDPOINTS.length; i++) {
  await payForEndpoint(ENDPOINTS[i], i);
}
console.log('\nDone! Check https://ssreeni1.github.io/paygate/marketplace.html');
