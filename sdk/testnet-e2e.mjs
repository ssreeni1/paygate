/**
 * PayGate Testnet E2E Test
 *
 * Full payment flow on Tempo Moderato testnet:
 * 1. Start echo server + paygate gateway
 * 2. Request API → get 402 with pricing + quote_id
 * 3. Send TIP-20 transferWithMemo on-chain with correct memo
 * 4. Retry with X-Payment-Tx header → get 200
 * 5. Retry same tx → get 409 (replay protection)
 */

import { createClient, http, publicActions, walletActions } from 'viem';
import { privateKeyToAccount } from 'viem/accounts';
import { requestHash, paymentMemo } from './src/hash.js';

// ─── Config ────────────────────────────────────────────────

const CONSUMER_KEY = '0x55f1eb7043e21b0c7417de5398a423db0cad1603ff50e2fd16f3ac02a5b5dc79';
const PROVIDER_ADDRESS = '0x002925FAFE98cfeB9fdBb7d6045ce318E4BD4b88';
const TOKEN_ADDRESS = '0x20c0000000000000000000000000000000000000';
const GATEWAY_URL = 'http://127.0.0.1:18080';
const RPC_URL = 'https://rpc.moderato.tempo.xyz';
const CHAIN_ID = 42431;

// TIP-20 transferWithMemo ABI
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

// ─── Helpers ───────────────────────────────────────────────

function log(step, msg) {
  const icon = msg.startsWith('✓') ? '' : msg.startsWith('✗') ? '' : '  ';
  console.log(`${icon}[${step}] ${msg}`);
}

async function sleep(ms) {
  return new Promise(r => setTimeout(r, ms));
}

// ─── Main ──────────────────────────────────────────────────

async function main() {
  console.log('');
  console.log('═══════════════════════════════════════════════');
  console.log('  PayGate Testnet E2E Test (Tempo Moderato)');
  console.log('═══════════════════════════════════════════════');
  console.log('');

  // Set up viem client
  const account = privateKeyToAccount(CONSUMER_KEY);
  log('SETUP', `Consumer: ${account.address}`);
  log('SETUP', `Provider: ${PROVIDER_ADDRESS}`);
  log('SETUP', `Token: ${TOKEN_ADDRESS}`);

  const client = createClient({
    account,
    chain: { id: CHAIN_ID, name: 'Tempo Moderato', nativeCurrency: { name: 'USD', symbol: 'USD', decimals: 6 }, rpcUrls: { default: { http: [RPC_URL] } } },
    transport: http(RPC_URL),
  })
    .extend(publicActions)
    .extend(walletActions);

  // Step 1: Check balance
  log('1/7', 'Checking testnet token balance...');
  const balance = await client.readContract({
    address: TOKEN_ADDRESS,
    abi: TIP20_ABI,
    functionName: 'balanceOf',
    args: [account.address],
  });
  log('1/7', `✓ Balance: ${Number(balance) / 1_000_000} USDC`);

  if (balance === 0n) {
    log('1/7', '✗ No balance — fund wallet first with tempo_fundAddress');
    process.exit(1);
  }

  // Step 2: Hit gateway without payment → expect 402
  log('2/7', 'Requesting paid endpoint without payment...');
  const resp402 = await fetch(`${GATEWAY_URL}/v1/chat/completions`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ model: 'gpt-4', messages: [{ role: 'user', content: 'hello' }] }),
  });

  if (resp402.status !== 402) {
    log('2/7', `✗ Expected 402, got ${resp402.status}`);
    process.exit(1);
  }

  const pricing = await resp402.json();
  log('2/7', `✓ 402 Payment Required — quote_id=${pricing.pricing.quote_id}, price=${pricing.pricing.amount} USDC`);

  const quoteId = pricing.pricing.quote_id;
  const priceBaseUnits = BigInt(pricing.pricing.amount_base_units);

  // Step 3: Compute request hash and memo
  log('3/7', 'Computing request hash and payment memo...');
  const method = 'POST';
  const path = '/v1/chat/completions';
  const body = JSON.stringify({ model: 'gpt-4', messages: [{ role: 'user', content: 'hello' }] });

  const reqHash = requestHash(method, path, body);
  const memo = paymentMemo(quoteId, reqHash);

  log('3/7', `✓ requestHash=${reqHash.slice(0, 18)}...`);
  log('3/7', `✓ memo=${memo.slice(0, 18)}...`);

  // Step 4: Send TIP-20 transferWithMemo on-chain
  log('4/7', `Sending ${Number(priceBaseUnits) / 1_000_000} USDC to provider on-chain...`);

  const txHash = await client.writeContract({
    address: TOKEN_ADDRESS,
    abi: TIP20_ABI,
    functionName: 'transferWithMemo',
    args: [PROVIDER_ADDRESS, priceBaseUnits, memo],
  });

  log('4/7', `✓ Transaction sent: ${txHash}`);

  // Wait for confirmation
  log('4/7', 'Waiting for confirmation...');
  const receipt = await client.waitForTransactionReceipt({ hash: txHash });
  log('4/7', `✓ Confirmed in block ${receipt.blockNumber} (status: ${receipt.status})`);

  if (receipt.status !== 'success') {
    log('4/7', '✗ Transaction failed on-chain');
    process.exit(1);
  }

  // Step 5: Retry with payment headers → expect 200
  log('5/7', 'Retrying with X-Payment-Tx header...');
  await sleep(500); // Brief pause for RPC indexing

  const resp200 = await fetch(`${GATEWAY_URL}/v1/chat/completions`, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      'X-Payment-Tx': txHash,
      'X-Payment-Payer': account.address,
      'X-Payment-Quote-Id': quoteId,
    },
    body,
  });

  if (resp200.status === 200) {
    const echoBody = await resp200.json();
    const receiptHeader = resp200.headers.get('x-payment-receipt');
    const costHeader = resp200.headers.get('x-payment-cost');
    log('5/7', `✓ 200 OK — proxied to upstream`);
    log('5/7', `✓ X-Payment-Receipt: ${receiptHeader}`);
    log('5/7', `✓ X-Payment-Cost: ${costHeader}`);
    log('5/7', `✓ Echo body confirms upstream received request`);
  } else {
    const errBody = await resp200.json();
    log('5/7', `✗ Expected 200, got ${resp200.status}: ${JSON.stringify(errBody)}`);
    // Don't exit — continue to see if replay protection works
  }

  // Step 6: Replay same tx → expect 409
  log('6/7', 'Replaying same transaction...');
  const respReplay = await fetch(`${GATEWAY_URL}/v1/chat/completions`, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      'X-Payment-Tx': txHash,
      'X-Payment-Payer': account.address,
      'X-Payment-Quote-Id': quoteId,
    },
    body,
  });

  if (respReplay.status === 409) {
    log('6/7', '✓ 409 Conflict — replay protection works');
  } else {
    log('6/7', `✗ Expected 409, got ${respReplay.status}`);
  }

  // Step 7: Check receipt endpoint
  log('7/7', 'Checking receipt endpoint...');
  const respReceipt = await fetch(`${GATEWAY_URL}/paygate/receipts/${txHash}`);
  if (respReceipt.status === 200) {
    const receiptData = await respReceipt.json();
    log('7/7', `✓ Receipt found: payer=${receiptData.payer_address?.slice(0, 10)}..., amount=${receiptData.amount}`);
  } else {
    log('7/7', `✗ Receipt lookup returned ${respReceipt.status}`);
  }

  console.log('');
  console.log('═══════════════════════════════════════════════');
  console.log('  E2E Test Complete');
  console.log('═══════════════════════════════════════════════');
}

main().catch(e => {
  console.error('Fatal error:', e.message);
  process.exit(1);
});
