#!/usr/bin/env node
/**
 * Agent Tips E2E Simulation
 *
 * Tests the full tipping flow: single tip, batch tip, npm resolution,
 * escrow, internal API, claim flow, leaderboard, and edge cases.
 *
 * Starts the gateway + demo server locally, runs all tests, produces
 * a structured JSON report.
 *
 * Usage:
 *   node tests/e2e/tips-sim.mjs
 *
 * Optional env vars:
 *   GATEWAY_URL   — default: http://127.0.0.1:8080
 *   SKIP_INFRA    — "true" to use running instance
 */

import { spawn } from 'child_process';
import { fileURLToPath } from 'url';
import { dirname, join } from 'path';
import { writeFileSync } from 'fs';

const __dirname = dirname(fileURLToPath(import.meta.url));
const ROOT = join(__dirname, '..', '..');

const GATEWAY_URL = process.env.GATEWAY_URL || 'http://127.0.0.1:8080';
const SKIP_INFRA = process.env.SKIP_INFRA === 'true';

// ─── Logging ───────────────────────────────────────────────────────
const log = [];
const startTime = Date.now();

const phaseResults = {
  single_tip: { tests: 0, passed: 0 },
  batch_tip: { tests: 0, passed: 0 },
  npm_resolution: { tests: 0, passed: 0 },
  internal_api: { tests: 0, passed: 0 },
  claim_flow: { tests: 0, passed: 0 },
  edge_cases: { tests: 0, passed: 0 },
};

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
  console.log(`  ${icon} [${(entry.elapsed_ms / 1000).toFixed(1)}s] ${name}`);
  if (status === 'FAIL' && data.error) {
    console.log(`    error: ${data.error}`);
  }
  return entry;
}

function recordTest(phase, passed) {
  phaseResults[phase].tests++;
  if (passed) phaseResults[phase].passed++;
}

// ─── Gateway API helpers ──────────────────────────────────────────
async function gw(path, opts = {}) {
  const url = `${GATEWAY_URL}${path}`;
  const resp = await fetch(url, {
    ...opts,
    headers: {
      'Content-Type': 'application/json',
      ...(opts.headers || {}),
    },
  });
  const text = await resp.text();
  let json;
  try { json = JSON.parse(text); } catch { json = null; }
  return { status: resp.status, headers: resp.headers, json, text };
}

async function tip(body) {
  return gw('/paygate/tip', {
    method: 'POST',
    body: JSON.stringify(body),
  });
}

async function tipBatch(body) {
  return gw('/paygate/tip/batch', {
    method: 'POST',
    body: JSON.stringify(body),
  });
}

async function internal(path, opts = {}) {
  return gw(`/paygate/internal${path}`, {
    ...opts,
    headers: {
      // No auth secret in dev mode = all requests pass
      ...(opts.headers || {}),
    },
  });
}

// ─── Infrastructure ───────────────────────────────────────────────
let gatewayProc, demoProc;

async function startInfra() {
  if (SKIP_INFRA) {
    step('Using existing gateway');
    return;
  }

  step('Starting demo echo server');
  demoProc = spawn('cargo', ['run', '--', 'demo'], {
    cwd: ROOT,
    stdio: 'pipe',
    env: { ...process.env, RUST_LOG: 'info' },
  });

  // Wait for gateway to be ready
  step('Waiting for gateway...');
  for (let i = 0; i < 30; i++) {
    try {
      const resp = await fetch(`${GATEWAY_URL}/paygate/health`);
      if (resp.ok) {
        step('Gateway ready', { status: 'PASS' });
        return;
      }
    } catch {}
    await new Promise(r => setTimeout(r, 1000));
  }
  step('Gateway failed to start', { status: 'FAIL', error: 'Timeout after 30s' });
  process.exit(1);
}

function stopInfra() {
  if (gatewayProc) gatewayProc.kill();
  if (demoProc) demoProc.kill();
}

// ─── Phase 1: Single Tip ─────────────────────────────────────────
async function phaseSingleTip() {
  step('Phase 1: Single Tip');

  // T1: Tip a known npm package (chalk)
  {
    const r = await tip({
      target: 'chalk',
      amount_usd: 0.50,
      reason: 'Color output in CLI tool',
      sender_name: 'test-agent',
    });
    const pass = r.status === 200 && r.json?.tip_id && r.json?.receipt_url;
    step('T1: Tip chalk by package name', {
      status: pass ? 'PASS' : 'FAIL',
      tip_id: r.json?.tip_id,
      resolved_github: r.json?.resolved_github,
      receipt_status: r.json?.status,
      error: pass ? undefined : `status=${r.status} body=${r.text?.slice(0, 200)}`,
    });
    recordTest('single_tip', pass);
  }

  // T2: Tip by GitHub username
  {
    const r = await tip({
      target: '@sindresorhus',
      amount_usd: 0.25,
      reason: 'Great open source work',
      sender_name: 'test-agent',
    });
    const pass = r.status === 200 && r.json?.resolved_github === 'sindresorhus';
    step('T2: Tip by @github_username', {
      status: pass ? 'PASS' : 'FAIL',
      resolved_github: r.json?.resolved_github,
      error: pass ? undefined : `status=${r.status} body=${r.text?.slice(0, 200)}`,
    });
    recordTest('single_tip', pass);
  }

  // T3: Tip with evidence (optional field)
  {
    const r = await tip({
      target: 'lodash',
      amount_usd: 0.10,
      reason: 'Utility functions',
      evidence: 'Used lodash.debounce in event handler',
      sender_name: 'test-agent',
    });
    const pass = r.status === 200 && r.json?.tip_id;
    step('T3: Tip with evidence field', {
      status: pass ? 'PASS' : 'FAIL',
      error: pass ? undefined : `status=${r.status}`,
    });
    recordTest('single_tip', pass);
  }

  // T4: Tip status should be escrowed (no wallet registered)
  {
    const r = await tip({
      target: '@nobody-has-this-wallet',
      amount_usd: 0.05,
      reason: 'Test escrow',
      sender_name: 'test-agent',
    });
    const pass = r.status === 200 && r.json?.status === 'escrowed';
    step('T4: Unregistered recipient gets escrowed', {
      status: pass ? 'PASS' : 'FAIL',
      tip_status: r.json?.status,
      error: pass ? undefined : `expected escrowed, got ${r.json?.status}`,
    });
    recordTest('single_tip', pass);
  }
}

// ─── Phase 2: Batch Tip ──────────────────────────────────────────
async function phaseBatchTip() {
  step('Phase 2: Batch Tip');

  // T5: Batch tip multiple packages
  {
    const r = await tipBatch({
      tips: [
        { target: 'express', amount_usd: 0.10, reason: 'Web framework' },
        { target: 'debug', amount_usd: 0.05, reason: 'Logging utility' },
        { target: 'commander', amount_usd: 0.10, reason: 'CLI parsing' },
      ],
      sender_name: 'batch-agent',
    });
    const pass = r.status === 200
      && r.json?.summary?.total === 3
      && r.json?.summary?.succeeded >= 2;
    step('T5: Batch tip 3 packages', {
      status: pass ? 'PASS' : 'FAIL',
      total: r.json?.summary?.total,
      succeeded: r.json?.summary?.succeeded,
      failed: r.json?.summary?.failed,
      error: pass ? undefined : `summary=${JSON.stringify(r.json?.summary)}`,
    });
    recordTest('batch_tip', pass);
  }

  // T6: Batch with duplicate packages should dedup
  {
    const r = await tipBatch({
      tips: [
        { target: 'minimist', amount_usd: 0.10, reason: 'Arg parsing' },
        { target: 'minimist', amount_usd: 0.10, reason: 'Arg parsing again' },
      ],
      sender_name: 'dedup-agent',
    });
    const hasDup = r.json?.results?.some(res => res.status === 'skipped_duplicate');
    const pass = r.status === 200 && hasDup;
    step('T6: Batch dedup same package', {
      status: pass ? 'PASS' : 'FAIL',
      results: r.json?.results?.map(res => `${res.target}: ${res.status}`),
      error: pass ? undefined : 'No dedup detected',
    });
    recordTest('batch_tip', pass);
  }

  // T7: Empty batch returns error
  {
    const r = await tipBatch({ tips: [], sender_name: 'empty-agent' });
    const pass = r.status === 400;
    step('T7: Empty batch returns 400', {
      status: pass ? 'PASS' : 'FAIL',
      http_status: r.status,
    });
    recordTest('batch_tip', pass);
  }
}

// ─── Phase 3: npm Resolution ─────────────────────────────────────
async function phaseNpmResolution() {
  step('Phase 3: npm Resolution');

  // T8: Package with no repository field
  {
    const r = await tip({
      target: 'nonexistent-pkg-zzz-12345',
      amount_usd: 0.01,
      reason: 'Test',
      sender_name: 'test-agent',
    });
    const pass = r.status === 404;
    step('T8: Unknown package returns 404', {
      status: pass ? 'PASS' : 'FAIL',
      http_status: r.status,
    });
    recordTest('npm_resolution', pass);
  }

  // T9: Cache hit on second request (same package)
  {
    const t1 = Date.now();
    await tip({ target: 'chalk', amount_usd: 0.01, reason: 'Cache test 1', sender_name: 'cache-agent' });
    const d1 = Date.now() - t1;

    const t2 = Date.now();
    await tip({ target: 'chalk', amount_usd: 0.01, reason: 'Cache test 2', sender_name: 'cache-agent' });
    const d2 = Date.now() - t2;

    // Second request should be faster (cache hit vs npm API call)
    const pass = d2 < d1 || d2 < 500; // Either faster or both fast (cache from T1)
    step('T9: npm cache hit on second request', {
      status: pass ? 'PASS' : 'FAIL',
      first_ms: d1,
      second_ms: d2,
    });
    recordTest('npm_resolution', pass);
  }
}

// ─── Phase 4: Internal API ───────────────────────────────────────
async function phaseInternalApi() {
  step('Phase 4: Internal API');

  // First, create a tip to query
  const createResp = await tip({
    target: '@internal-test-user',
    amount_usd: 0.50,
    reason: 'Internal API test',
    sender_name: 'internal-agent',
  });
  const tipId = createResp.json?.tip_id;

  // T10: Get single tip by ID
  {
    const r = await internal(`/tips/${tipId}`);
    const pass = r.status === 200 && r.json?.id === tipId && r.json?.amount_usdc === 500000;
    step('T10: GET /internal/tips/:id', {
      status: pass ? 'PASS' : 'FAIL',
      tip_id: tipId,
      amount_usdc: r.json?.amount_usdc,
      error: pass ? undefined : `status=${r.status}`,
    });
    recordTest('internal_api', pass);
  }

  // T11: Get tips by recipient
  {
    const r = await internal('/tips/by-recipient/internal-test-user');
    const pass = r.status === 200 && Array.isArray(r.json) && r.json.length >= 1;
    step('T11: GET /internal/tips/by-recipient/:gh', {
      status: pass ? 'PASS' : 'FAIL',
      count: r.json?.length,
    });
    recordTest('internal_api', pass);
  }

  // T12: Get leaderboard
  {
    const r = await internal('/leaderboard');
    const pass = r.status === 200 && Array.isArray(r.json);
    step('T12: GET /internal/leaderboard', {
      status: pass ? 'PASS' : 'FAIL',
      entries: r.json?.length,
    });
    recordTest('internal_api', pass);
  }

  // T13: Get non-existent tip returns 404
  {
    const r = await internal('/tips/tip_nonexistent_000');
    const pass = r.status === 404;
    step('T13: Non-existent tip returns 404', {
      status: pass ? 'PASS' : 'FAIL',
      http_status: r.status,
    });
    recordTest('internal_api', pass);
  }
}

// ─── Phase 5: Claim Flow ─────────────────────────────────────────
async function phaseClaimFlow() {
  step('Phase 5: Claim Flow');

  // Use a unique username per run to avoid stale wallet registrations
  const claimUser = `claim-test-${Date.now()}`;

  // Create an escrowed tip to claim
  const createClaimTip = await tip({
    target: `@${claimUser}`,
    amount_usd: 0.50,
    reason: 'Claim test',
    sender_name: 'claim-agent',
  });
  step('Setup: created tip for claim test', {
    tip_id: createClaimTip.json?.tip_id,
    status: createClaimTip.json?.status,
    user: claimUser,
  });

  // T14: Claim tips for a user
  {
    const r = await internal('/claim', {
      method: 'POST',
      body: JSON.stringify({
        github_username: claimUser,
        wallet_address: '0x742d35Cc6634C0532925a3b844Bc9e7595f3fAE6',
      }),
    });
    const pass = r.status === 200 && r.json?.claimed >= 1;
    step('T14: Claim escrowed tips', {
      status: pass ? 'PASS' : 'FAIL',
      claimed: r.json?.claimed,
      wallet: r.json?.wallet,
    });
    recordTest('claim_flow', pass);
  }

  // T15: Claim with no pending tips
  {
    const r = await internal('/claim', {
      method: 'POST',
      body: JSON.stringify({
        github_username: 'nobody-has-tips',
        wallet_address: '0x0000000000000000000000000000000000000001',
      }),
    });
    const pass = r.status === 404;
    step('T15: Claim with no pending tips returns 404', {
      status: pass ? 'PASS' : 'FAIL',
      http_status: r.status,
    });
    recordTest('claim_flow', pass);
  }
}

// ─── Phase 6: Edge Cases ─────────────────────────────────────────
async function phaseEdgeCases() {
  step('Phase 6: Edge Cases');

  // T16: Amount below minimum
  {
    const r = await tip({
      target: '@someone',
      amount_usd: 0.001,
      reason: 'Too small',
      sender_name: 'edge-agent',
    });
    const pass = r.status === 400;
    step('T16: Below min amount returns 400', {
      status: pass ? 'PASS' : 'FAIL',
      http_status: r.status,
    });
    recordTest('edge_cases', pass);
  }

  // T17: Amount above maximum
  {
    const r = await tip({
      target: '@someone',
      amount_usd: 999.99,
      reason: 'Too much',
      sender_name: 'edge-agent',
    });
    const pass = r.status === 400;
    step('T17: Above max amount returns 400', {
      status: pass ? 'PASS' : 'FAIL',
      http_status: r.status,
    });
    recordTest('edge_cases', pass);
  }

  // T18: XSS in reason field
  {
    const r = await tip({
      target: '@xss-test',
      amount_usd: 0.05,
      reason: '<script>alert("xss")</script>',
      sender_name: 'xss-agent',
    });
    // Should succeed but reason should be sanitized
    if (r.status === 200 && r.json?.tip_id) {
      const tipResp = await internal(`/tips/${r.json.tip_id}`);
      const sanitized = tipResp.json?.reason || '';
      const pass = !sanitized.includes('<script>') && sanitized.includes('&lt;script&gt;');
      step('T18: XSS in reason is sanitized', {
        status: pass ? 'PASS' : 'FAIL',
        sanitized_reason: sanitized.slice(0, 60),
      });
      recordTest('edge_cases', pass);
    } else {
      step('T18: XSS tip creation failed', { status: 'FAIL', http_status: r.status });
      recordTest('edge_cases', false);
    }
  }

  // T19: Very long reason truncated
  {
    const longReason = 'x'.repeat(1000);
    const r = await tip({
      target: '@truncate-test',
      amount_usd: 0.05,
      reason: longReason,
      sender_name: 'truncate-agent',
    });
    if (r.status === 200 && r.json?.tip_id) {
      const tipResp = await internal(`/tips/${r.json.tip_id}`);
      const stored = tipResp.json?.reason || '';
      const pass = stored.length <= 500;
      step('T19: Long reason is truncated', {
        status: pass ? 'PASS' : 'FAIL',
        stored_length: stored.length,
      });
      recordTest('edge_cases', pass);
    } else {
      step('T19: Truncation tip failed', { status: 'FAIL' });
      recordTest('edge_cases', false);
    }
  }
}

// ─── Main ─────────────────────────────────────────────────────────
async function main() {
  console.log();
  console.log('  ─── Agent Tips E2E Simulation ───');
  console.log();

  try {
    await startInfra();

    await phaseSingleTip();
    await phaseBatchTip();
    await phaseNpmResolution();
    await phaseInternalApi();
    await phaseClaimFlow();
    await phaseEdgeCases();

  } catch (err) {
    step('FATAL', { status: 'FAIL', error: err.message });
  } finally {
    stopInfra();
  }

  // ─── Report ───────────────────────────────────────────────────
  console.log();
  console.log('  ─── Results ───');
  console.log();

  let totalTests = 0;
  let totalPassed = 0;
  for (const [phase, results] of Object.entries(phaseResults)) {
    const icon = results.passed === results.tests ? '✓' : '✗';
    console.log(`  ${icon} ${phase}: ${results.passed}/${results.tests}`);
    totalTests += results.tests;
    totalPassed += results.passed;
  }

  console.log();
  console.log(`  Total: ${totalPassed}/${totalTests} passed`);
  console.log();

  // Write JSON report
  const report = {
    version: '0.6.0',
    date: new Date().toISOString().slice(0, 10),
    duration_ms: Date.now() - startTime,
    phases: phaseResults,
    total: { tests: totalTests, passed: totalPassed },
    log,
  };

  const reportPath = join(__dirname, `tips-sim-${report.date}.json`);
  writeFileSync(reportPath, JSON.stringify(report, null, 2));
  console.log(`  Report: ${reportPath}`);
  console.log();

  process.exit(totalPassed === totalTests ? 0 : 1);
}

main();
