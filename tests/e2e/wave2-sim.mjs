#!/usr/bin/env node
/**
 * Wave 2 E2E Simulation — Sessions, Dynamic Pricing, Fee Sponsorship
 *
 * Spins up the gateway + demo server, funds a wallet on Tempo testnet,
 * runs full agent workflows, and produces a structured log with explorer links.
 *
 * Usage:
 *   PAYGATE_PRIVATE_KEY=0x... node tests/e2e/wave2-sim.mjs
 *
 * Optional env vars:
 *   GATEWAY_URL      — default: http://localhost:8080
 *   TEMPO_RPC_URL    — default: https://rpc.moderato.tempo.xyz
 *   SKIP_INFRA       — set to "true" to skip starting gateway/demo (use running instance)
 */

import { createHmac } from 'crypto';
import { spawn } from 'child_process';
import { fileURLToPath } from 'url';
import { dirname, join } from 'path';

const __dirname = dirname(fileURLToPath(import.meta.url));
const ROOT = join(__dirname, '..', '..');

// ─── Config ────────────────────────────────────────────────────────
const GATEWAY_URL = process.env.GATEWAY_URL || 'http://localhost:8080';
const TEMPO_RPC = process.env.TEMPO_RPC_URL || 'https://rpc.moderato.tempo.xyz';
const CHAIN_ID = 42431;
const PATHUSD = '0x20c0000000000000000000000000000000000000';
const PROVIDER = '0x002925FAFE98cfeB9fdBb7d6045ce318E4BD4b88';
const EXPLORER = 'https://explore.moderato.tempo.xyz';
const SKIP_INFRA = process.env.SKIP_INFRA === 'true';

// ─── Logging ───────────────────────────────────────────────────────
const log = [];
const startTime = Date.now();

function step(name, data = {}) {
  const entry = {
    step: name,
    timestamp: new Date().toISOString(),
    elapsed_ms: Date.now() - startTime,
    ...data,
  };
  log.push(entry);
  const status = data.status || 'INFO';
  const icon = status === 'PASS' ? '✓' : status === 'FAIL' ? '✗' : '→';
  const extra = data.tx_hash ? ` | ${EXPLORER}/tx/${data.tx_hash}` : '';
  const balance = data.balance_after !== undefined ? ` | balance: $${(data.balance_after / 1_000_000).toFixed(6)}` : '';
  console.log(`  ${icon} [${(entry.elapsed_ms / 1000).toFixed(1)}s] ${name}${extra}${balance}`);
  if (status === 'FAIL' && data.error) {
    console.log(`    error: ${data.error}`);
  }
  return entry;
}

function explorerLink(txHash) {
  return `${EXPLORER}/tx/${txHash}`;
}

// ─── RPC helpers ───────────────────────────────────────────────────
async function rpcCall(method, params = []) {
  const resp = await fetch(TEMPO_RPC, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ jsonrpc: '2.0', method, params, id: 1 }),
  });
  const data = await resp.json();
  if (data.error) throw new Error(`RPC ${method}: ${JSON.stringify(data.error)}`);
  return data.result;
}

async function getBalance(address) {
  const result = await rpcCall('eth_getBalance', [address, 'latest']);
  return BigInt(result);
}

async function getTokenBalance(token, address) {
  // balanceOf(address) selector = 0x70a08231
  const data = '0x70a08231' + address.slice(2).padStart(64, '0');
  const result = await rpcCall('eth_call', [{ to: token, data }, 'latest']);
  return BigInt(result);
}

async function fundAddress(address) {
  return rpcCall('tempo_fundAddress', [address]);
}

// ─── Crypto helpers ────────────────────────────────────────────────
// Dynamic import for viem (ESM)
let keccak256, toBytes, toHex, privateKeyToAccount, createWalletClient, createPublicClient, http, parseAbi, encodeFunctionData;

async function loadViem() {
  const viem = await import('viem');
  keccak256 = viem.keccak256;
  toBytes = viem.toBytes;
  toHex = viem.toHex;
  parseAbi = viem.parseAbi;
  encodeFunctionData = viem.encodeFunctionData;

  const accounts = await import('viem/accounts');
  privateKeyToAccount = accounts.privateKeyToAccount;

  const clients = await import('viem');
  createWalletClient = clients.createWalletClient;
  createPublicClient = clients.createPublicClient;
  http = clients.http;
}

function requestHash(method, path, body) {
  const encoder = new TextEncoder();
  const input = new Uint8Array([
    ...encoder.encode(method),
    0x20, // space
    ...encoder.encode(path),
    0x0a, // newline
    ...encoder.encode(body),
  ]);
  return keccak256(input);
}

function sessionMemo(nonce) {
  const input = new TextEncoder().encode('paygate-session' + nonce);
  return keccak256(input);
}

function hmacSha256(secret, message) {
  const rawSecret = secret.startsWith('ssec_') ? secret.slice(5) : secret;
  return createHmac('sha256', Buffer.from(rawSecret, 'hex'))
    .update(message)
    .digest('hex');
}

// ─── Gateway API helpers ───────────────────────────────────────────
async function gw(path, opts = {}) {
  const url = `${GATEWAY_URL}${path}`;
  const resp = await fetch(url, opts);
  const text = await resp.text();
  let json;
  try { json = JSON.parse(text); } catch { json = null; }
  return { status: resp.status, headers: resp.headers, json, text };
}

// ─── Infrastructure ────────────────────────────────────────────────
let gatewayProc, demoProc;

async function startInfra() {
  if (SKIP_INFRA) {
    step('infra.skip', { status: 'INFO', message: 'SKIP_INFRA=true, using running instance' });
    return;
  }

  // Start demo server
  demoProc = spawn('node', ['dist/server.js'], {
    cwd: join(ROOT, 'demo'),
    env: { ...process.env, DEMO_PORT: '3001' },
    stdio: ['ignore', 'pipe', 'pipe'],
  });

  // Start gateway
  gatewayProc = spawn('cargo', ['run', '-p', 'paygate-gateway', '--', 'serve', '-c', join(ROOT, 'demo', 'paygate.toml')], {
    cwd: ROOT,
    env: { ...process.env },
    stdio: ['ignore', 'pipe', 'pipe'],
  });

  // Wait for both to be ready
  await new Promise(r => setTimeout(r, 5000));
  step('infra.start', { status: 'PASS', message: 'Gateway + demo server started' });
}

function stopInfra() {
  if (gatewayProc) gatewayProc.kill();
  if (demoProc) demoProc.kill();
}

// ─── Test: Health Check ────────────────────────────────────────────
async function testHealthCheck() {
  try {
    const resp = await fetch(`${GATEWAY_URL}/paygate/health`);
    const data = await resp.json();
    step('health.check', { status: resp.ok ? 'PASS' : 'FAIL', response: data });
    return resp.ok;
  } catch (e) {
    step('health.check', { status: 'FAIL', error: e.message });
    return false;
  }
}

// ─── Test: 402 Negotiation ─────────────────────────────────────────
async function test402Negotiation() {
  const { status, json } = await gw('/v1/search', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ query: 'test' }),
  });

  const pass = status === 402 && json?.pricing?.amount;
  step('402.negotiation', {
    status: pass ? 'PASS' : 'FAIL',
    http_status: status,
    price: json?.pricing?.amount,
    token: json?.pricing?.token,
    recipient: json?.pricing?.recipient,
    dynamic: json?.pricing?.dynamic,
  });
  return { pass, pricing: json?.pricing };
}

// ─── Test: Session Workflow ────────────────────────────────────────
async function testSessionWorkflow(account) {
  const payer = account.address;

  // Step 1: Get nonce
  const nonceResp = await gw('/paygate/sessions/nonce', {
    method: 'POST',
    headers: { 'X-Payment-Payer': payer },
  });

  if (nonceResp.status !== 200) {
    step('session.nonce', { status: 'FAIL', error: `HTTP ${nonceResp.status}: ${nonceResp.text}` });
    return null;
  }

  const { nonce } = nonceResp.json;
  step('session.nonce', { status: 'PASS', nonce });

  // Step 2: Deposit on-chain
  const depositAmount = 50000n; // 0.05 USDC (50000 base units)
  const memo = sessionMemo(nonce);

  const TIP20_ABI = parseAbi([
    'function transferWithMemo(address to, uint256 amount, bytes32 memo) returns (bool)',
  ]);

  const chain = {
    id: CHAIN_ID,
    name: 'Tempo Moderato',
    nativeCurrency: { name: 'TEMPO', symbol: 'TEMPO', decimals: 18 },
    rpcUrls: { default: { http: [TEMPO_RPC] } },
  };

  const walletClient = createWalletClient({
    account,
    chain,
    transport: http(TEMPO_RPC),
  });

  const publicClient = createPublicClient({
    chain,
    transport: http(TEMPO_RPC),
  });

  let txHash;
  try {
    txHash = await walletClient.sendTransaction({
      to: PATHUSD,
      data: encodeFunctionData({
        abi: TIP20_ABI,
        functionName: 'transferWithMemo',
        args: [PROVIDER, depositAmount, memo],
      }),
    });

    step('session.deposit', { status: 'PASS', tx_hash: txHash, amount: depositAmount.toString() });

    // Wait for receipt
    const receipt = await publicClient.waitForTransactionReceipt({ hash: txHash });
    step('session.deposit.confirmed', {
      status: receipt.status === 'success' ? 'PASS' : 'FAIL',
      block: Number(receipt.blockNumber),
      tx_hash: txHash,
    });
  } catch (e) {
    step('session.deposit', { status: 'FAIL', error: e.message });
    return null;
  }

  // Step 3: Create session
  const sessionResp = await gw('/paygate/sessions', {
    method: 'POST',
    headers: {
      'X-Payment-Tx': txHash,
      'X-Payment-Payer': payer,
      'Content-Type': 'application/json',
    },
    body: JSON.stringify({ nonce }),
  });

  if (sessionResp.status !== 201) {
    step('session.create', { status: 'FAIL', http_status: sessionResp.status, error: sessionResp.text });
    return null;
  }

  const session = sessionResp.json;
  step('session.create', {
    status: 'PASS',
    session_id: session.sessionId,
    balance: session.balance,
    rate: session.ratePerRequest,
    expires_at: session.expiresAt,
  });

  return session;
}

// ─── Test: HMAC-Authenticated Requests ─────────────────────────────
async function testSessionRequests(session, numCalls = 5) {
  const results = [];

  for (let i = 0; i < numCalls; i++) {
    const method = 'POST';
    const path = '/v1/search';
    const body = JSON.stringify({ query: `test query ${i}` });

    const rh = requestHash(method, path, body);
    const ts = Math.floor(Date.now() / 1000).toString();
    const sig = hmacSha256(session.sessionSecret, rh + ts);

    const resp = await gw(path, {
      method,
      headers: {
        'Content-Type': 'application/json',
        'X-Payment-Session': session.sessionId,
        'X-Payment-Session-Sig': sig,
        'X-Payment-Timestamp': ts,
      },
      body,
    });

    const costHeader = resp.headers.get('x-payment-cost');
    const tokenCount = resp.headers.get('x-token-count');

    results.push({
      call: i + 1,
      status: resp.status,
      cost: costHeader,
      tokens: tokenCount,
    });

    step(`session.call.${i + 1}`, {
      status: resp.status === 200 ? 'PASS' : 'FAIL',
      http_status: resp.status,
      cost: costHeader,
      tokens: tokenCount,
    });
  }

  return results;
}

// ─── Test: Dynamic Pricing ─────────────────────────────────────────
async function testDynamicPricing(session) {
  const method = 'POST';
  const path = '/v1/summarize';
  const body = JSON.stringify({ url: 'https://example.com', max_length: 100 });

  const rh = requestHash(method, path, body);
  const ts = Math.floor(Date.now() / 1000).toString();
  const sig = hmacSha256(session.sessionSecret, rh + ts);

  const resp = await gw(path, {
    method,
    headers: {
      'Content-Type': 'application/json',
      'X-Payment-Session': session.sessionId,
      'X-Payment-Session-Sig': sig,
      'X-Payment-Timestamp': ts,
    },
    body,
  });

  const costHeader = resp.headers.get('x-payment-cost');
  const tokenCount = resp.headers.get('x-token-count');
  const hasDynamicCost = costHeader && tokenCount;

  step('dynamic_pricing.call', {
    status: resp.status === 200 ? 'PASS' : 'FAIL',
    http_status: resp.status,
    cost: costHeader,
    tokens: tokenCount,
    has_dynamic_cost: !!hasDynamicCost,
  });

  // Verify the cost was computed from token count
  if (tokenCount && costHeader) {
    const tokens = parseInt(tokenCount);
    const expectedCost = tokens * (0.00001 + 0.000005); // base + spread from config
    const actualCost = parseFloat(costHeader);
    const withinTolerance = Math.abs(actualCost - expectedCost) < 0.001;

    step('dynamic_pricing.verify', {
      status: withinTolerance ? 'PASS' : 'FAIL',
      tokens,
      expected_cost: expectedCost.toFixed(6),
      actual_cost: actualCost,
      tolerance: '< $0.001',
    });
  }

  return resp;
}

// ─── Test: Session Balance Widget ──────────────────────────────────
async function testSessionBalanceEndpoint(payer) {
  const resp = await gw(`/paygate/sessions?payer=${payer}`);

  const hasSessions = resp.json?.sessions?.length > 0;
  const session = resp.json?.sessions?.[0];

  step('balance_widget.endpoint', {
    status: resp.status === 200 ? 'PASS' : 'FAIL',
    active_sessions: resp.json?.sessions?.length || 0,
    balance: session?.balance,
    requests_made: session?.requestsMade,
  });

  return resp.json;
}

// ─── Test: Free Endpoint (no payment) ──────────────────────────────
async function testFreeEndpoint() {
  const resp = await gw('/v1/pricing');
  const pass = resp.status === 200 && resp.json?.endpoints;

  step('free_endpoint', {
    status: pass ? 'PASS' : 'FAIL',
    http_status: resp.status,
    endpoints: resp.json?.endpoints ? Object.keys(resp.json.endpoints).length : 0,
  });

  return pass;
}

// ─── Test: Session Exhaustion ──────────────────────────────────────
async function testSessionExhaustion(session) {
  // Keep calling until balance runs out
  let exhausted = false;
  let calls = 0;
  const maxCalls = 100; // safety cap

  while (!exhausted && calls < maxCalls) {
    const method = 'POST';
    const path = '/v1/search';
    const body = JSON.stringify({ query: `exhaust ${calls}` });

    const rh = requestHash(method, path, body);
    const ts = Math.floor(Date.now() / 1000).toString();
    const sig = hmacSha256(session.sessionSecret, rh + ts);

    const resp = await gw(path, {
      method,
      headers: {
        'Content-Type': 'application/json',
        'X-Payment-Session': session.sessionId,
        'X-Payment-Session-Sig': sig,
        'X-Payment-Timestamp': ts,
      },
      body,
    });

    calls++;

    if (resp.status === 402) {
      exhausted = true;
      const isBalanceError = resp.json?.error === 'insufficient_session_balance';
      step('session.exhaustion', {
        status: isBalanceError ? 'PASS' : 'FAIL',
        calls_made: calls,
        error: resp.json?.error,
        remaining_balance: resp.json?.balance,
      });
    }
  }

  if (!exhausted) {
    step('session.exhaustion', { status: 'FAIL', error: `Not exhausted after ${maxCalls} calls` });
  }

  return { calls, exhausted };
}

// ─── Main ──────────────────────────────────────────────────────────
async function main() {
  console.log();
  console.log('  PayGate Wave 2 E2E Simulation');
  console.log('  ─────────────────────────────');
  console.log();

  await loadViem();

  // Validate env
  const privateKey = process.env.PAYGATE_PRIVATE_KEY;
  if (!privateKey) {
    console.error('  error: PAYGATE_PRIVATE_KEY env var required');
    console.error('    hint: export PAYGATE_PRIVATE_KEY=0x<your-testnet-private-key>');
    process.exit(1);
  }

  const account = privateKeyToAccount(privateKey);
  step('init', { status: 'INFO', payer: account.address, gateway: GATEWAY_URL, rpc: TEMPO_RPC });

  try {
    // Phase 0: Start infrastructure
    await startInfra();

    // Phase 1: Health + connectivity
    const healthy = await testHealthCheck();
    if (!healthy && !SKIP_INFRA) {
      step('abort', { status: 'FAIL', error: 'Gateway unhealthy' });
      return;
    }

    // Phase 2: Check balances
    const nativeBalance = await getBalance(account.address);
    const tokenBalance = await getTokenBalance(PATHUSD, account.address);
    step('balances.initial', {
      status: 'INFO',
      native: `${(Number(nativeBalance) / 1e18).toFixed(4)} TEMPO`,
      usdc: `${(Number(tokenBalance) / 1e6).toFixed(6)} USDC`,
    });

    // Fund if needed
    if (nativeBalance < 1000000000000000n) { // < 0.001 TEMPO
      step('faucet.funding', { status: 'INFO', message: 'Low native balance, requesting from faucet' });
      await fundAddress(account.address);
      await new Promise(r => setTimeout(r, 3000));
      const newBalance = await getBalance(account.address);
      step('faucet.funded', { status: 'PASS', native: `${(Number(newBalance) / 1e18).toFixed(4)} TEMPO` });
    }

    if (tokenBalance < 100000n) { // < 0.1 USDC
      step('usdc.low', { status: 'FAIL', error: `USDC balance too low: ${Number(tokenBalance)} base units. Need at least 100000 (0.1 USDC). Fund the wallet at ${account.address}` });
      return;
    }

    // Phase 3: 402 negotiation
    const negotiation = await test402Negotiation();

    // Phase 4: Free endpoint
    await testFreeEndpoint();

    // Phase 5: Session workflow (the big one)
    const session = await testSessionWorkflow(account);
    if (!session) {
      step('session.abort', { status: 'FAIL', error: 'Session creation failed, cannot continue' });
      return;
    }

    // Phase 6: HMAC-authenticated requests
    await testSessionRequests(session, 5);

    // Phase 7: Dynamic pricing
    await testDynamicPricing(session);

    // Phase 8: Session balance endpoint
    await testSessionBalanceEndpoint(account.address);

    // Phase 9: Session exhaustion
    await testSessionExhaustion(session);

    // Phase 10: Final balance check
    const finalTokenBalance = await getTokenBalance(PATHUSD, account.address);
    step('balances.final', {
      status: 'INFO',
      usdc_before: `${(Number(tokenBalance) / 1e6).toFixed(6)} USDC`,
      usdc_after: `${(Number(finalTokenBalance) / 1e6).toFixed(6)} USDC`,
      usdc_spent: `${((Number(tokenBalance) - Number(finalTokenBalance)) / 1e6).toFixed(6)} USDC`,
    });

  } catch (e) {
    step('fatal', { status: 'FAIL', error: e.message, stack: e.stack?.split('\n').slice(0, 3).join(' | ') });
  } finally {
    stopInfra();
  }

  // ─── Report ────────────────────────────────────────────────────
  console.log();
  console.log('  ═══════════════════════════════════════════════════════');
  console.log('  SIMULATION REPORT');
  console.log('  ═══════════════════════════════════════════════════════');

  const passes = log.filter(e => e.status === 'PASS').length;
  const fails = log.filter(e => e.status === 'FAIL').length;
  const total = passes + fails;

  console.log(`  Result: ${fails === 0 ? 'ALL PASS' : `${fails} FAILURES`} (${passes}/${total})`);
  console.log(`  Duration: ${((Date.now() - startTime) / 1000).toFixed(1)}s`);

  const txEntries = log.filter(e => e.tx_hash);
  if (txEntries.length > 0) {
    console.log('  Explorer links:');
    for (const e of txEntries) {
      console.log(`    ${e.step}: ${explorerLink(e.tx_hash)}`);
    }
  }

  console.log('  ═══════════════════════════════════════════════════════');
  console.log();

  // Write JSON log
  const logPath = join(ROOT, 'tests', 'e2e', `wave2-sim-${new Date().toISOString().slice(0, 10)}.json`);
  const { writeFileSync } = await import('fs');
  writeFileSync(logPath, JSON.stringify({ report: { passes, fails, total, duration_ms: Date.now() - startTime }, steps: log }, null, 2));
  console.log(`  Log saved: ${logPath}`);
  console.log();

  process.exit(fails > 0 ? 1 : 0);
}

main().catch(e => {
  console.error('Fatal:', e);
  process.exit(1);
});
