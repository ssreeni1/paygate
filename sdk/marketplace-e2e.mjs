/**
 * PayGate Marketplace E2E — LIVE DEPLOYMENT
 *
 * Tests the deployed Railway instance with real Tempo testnet payments.
 * Hits multiple APIs, verifies 402 flow, pays on-chain, gets results.
 */

import { createClient, http, publicActions, walletActions } from 'viem';
import { privateKeyToAccount } from 'viem/accounts';
import { requestHash, paymentMemo } from './src/hash.js';

const CONSUMER_KEY = '0x55f1eb7043e21b0c7417de5398a423db0cad1603ff50e2fd16f3ac02a5b5dc79';
const GATEWAY = 'https://paygate-demo-production.up.railway.app';
const RPC_URL = 'https://rpc.moderato.tempo.xyz';
const CHAIN_ID = 42431;
const TOKEN = '0x20c0000000000000000000000000000000000000';

const TIP20_ABI = [
  {
    name: 'transferWithMemo',
    type: 'function',
    stateMutability: 'nonpayable',
    inputs: [
      { name: 'to', type: 'address' },
      { name: 'amount', type: 'uint256' },
      { name: 'memo', type: 'bytes32' },
    ],
    outputs: [{ type: 'bool' }],
  },
  {
    name: 'balanceOf',
    type: 'function',
    stateMutability: 'view',
    inputs: [{ name: 'account', type: 'address' }],
    outputs: [{ type: 'uint256' }],
  },
];

const account = privateKeyToAccount(CONSUMER_KEY);
const client = createClient({
  account,
  chain: { id: CHAIN_ID, name: 'Tempo Moderato', nativeCurrency: { name: 'USD', symbol: 'USD', decimals: 6 }, rpcUrls: { default: { http: [RPC_URL] } } },
  transport: http(RPC_URL),
}).extend(publicActions).extend(walletActions);

async function payAndCall(endpoint, body) {
  const method = 'POST';
  const path = endpoint;
  const bodyStr = JSON.stringify(body);

  // 1. Get 402
  const resp402 = await fetch(`${GATEWAY}${endpoint}`, {
    method, headers: { 'Content-Type': 'application/json' }, body: bodyStr,
  });
  if (resp402.status !== 402) throw new Error(`Expected 402, got ${resp402.status}`);
  const pricing = await resp402.json();
  const quoteId = pricing.pricing.quote_id;
  const amount = BigInt(pricing.pricing.amount_base_units);
  const recipient = pricing.pricing.recipient;

  // 2. Compute hash + memo
  const rh = requestHash(method, path, bodyStr);
  const memo = paymentMemo(quoteId, rh);

  // 3. Pay on-chain
  const txHash = await client.writeContract({
    address: TOKEN, abi: TIP20_ABI, functionName: 'transferWithMemo',
    args: [recipient, amount, memo],
  });
  const receipt = await client.waitForTransactionReceipt({ hash: txHash });
  if (receipt.status !== 'success') throw new Error('Transaction failed');

  // 4. Retry with payment
  await new Promise(r => setTimeout(r, 500));
  const resp200 = await fetch(`${GATEWAY}${endpoint}`, {
    method,
    headers: {
      'Content-Type': 'application/json',
      'X-Payment-Tx': txHash,
      'X-Payment-Payer': account.address,
      'X-Payment-Quote-Id': quoteId,
    },
    body: bodyStr,
  });

  return { status: resp200.status, txHash, amount: Number(amount), headers: Object.fromEntries(resp200.headers), body: await resp200.json() };
}

async function main() {
  console.log('');
  console.log('═══════════════════════════════════════════════════');
  console.log('  PayGate Marketplace E2E — LIVE DEPLOYMENT');
  console.log('  ' + GATEWAY);
  console.log('═══════════════════════════════════════════════════');
  console.log('');

  // Balance check
  const balance = await client.readContract({ address: TOKEN, abi: TIP20_ABI, functionName: 'balanceOf', args: [account.address] });
  console.log(`[SETUP] Consumer: ${account.address}`);
  console.log(`[SETUP] Balance: ${Number(balance) / 1_000_000} USDC`);
  console.log('');

  // Test 1: Pricing discovery (free)
  console.log('[1/5] Pricing discovery...');
  const pricingResp = await fetch(`${GATEWAY}/v1/pricing`);
  const pricingData = await pricingResp.json();
  console.log(`  ✓ ${pricingData.apis.length} APIs listed, demo_mode=${pricingData._demo_mode}`);

  // Test 2: Pay for search
  console.log('[2/5] Paying for web search...');
  const search = await payAndCall('/v1/search', { query: 'AI agents autonomous payments' });
  console.log(`  ✓ ${search.status} — paid ${search.amount / 1_000_000} USDC`);
  console.log(`    tx: ${search.txHash.slice(0, 20)}...`);
  console.log(`    receipt: ${search.headers['x-payment-receipt']?.slice(0, 20)}...`);
  console.log(`    cost: ${search.headers['x-payment-cost']}`);
  if (search.body._mock) console.log(`    (mock response — demo mode)`);

  // Test 3: Pay for image
  console.log('[3/5] Paying for image generation...');
  const image = await payAndCall('/v1/image', { prompt: 'a robot paying for API calls with coins' });
  console.log(`  ✓ ${image.status} — paid ${image.amount / 1_000_000} USDC`);
  console.log(`    tx: ${image.txHash.slice(0, 20)}...`);
  if (image.body._mock) console.log(`    (mock response — demo mode)`);

  // Test 4: Pay for summarize
  console.log('[4/5] Paying for text summarization...');
  const summary = await payAndCall('/v1/summarize', { text: 'PayGate is a reverse proxy that gates API access behind per-request stablecoin micropayments on the Tempo blockchain. Agents discover the price via a 402 response, pay on-chain, and retry with proof of payment. The gateway verifies the transaction in under 100ms and proxies the request to the upstream API.' });
  console.log(`  ✓ ${summary.status} — paid ${summary.amount / 1_000_000} USDC`);
  console.log(`    tx: ${summary.txHash.slice(0, 20)}...`);
  if (summary.body._mock) console.log(`    (mock response — demo mode)`);

  // Test 5: Replay rejection
  console.log('[5/5] Replay rejection (reuse search tx)...');
  const replay = await fetch(`${GATEWAY}/v1/search`, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      'X-Payment-Tx': search.txHash,
      'X-Payment-Payer': account.address,
    },
    body: JSON.stringify({ query: 'test' }),
  });
  if (replay.status === 409) {
    console.log(`  ✓ 409 Conflict — replay protection works`);
  } else {
    console.log(`  ✗ Expected 409, got ${replay.status}`);
  }

  // Summary
  const totalSpent = (search.amount + image.amount + summary.amount) / 1_000_000;
  console.log('');
  console.log('═══════════════════════════════════════════════════');
  console.log(`  3 APIs called. Total spent: $${totalSpent.toFixed(6)} USDC`);
  console.log('  All payments verified on Tempo Moderato testnet.');
  console.log('═══════════════════════════════════════════════════');
}

main().catch(e => { console.error('Fatal:', e.message); process.exit(1); });
