// Usage: TEMPO_PRIVATE_KEY=0x... GATEWAY_URL=http://localhost:8080 node sdk/sponsor-e2e.mjs

import { createClient, http, publicActions, walletActions, encodeFunctionData } from 'viem';
import { Account, tempoActions, withFeePayer } from 'viem/tempo';
import { tempoModerato } from 'viem/chains';

const GATEWAY_URL = process.env.GATEWAY_URL || 'http://localhost:8080';
const PRIVATE_KEY = process.env.TEMPO_PRIVATE_KEY;

if (!PRIVATE_KEY) {
  console.error('error: TEMPO_PRIVATE_KEY env var required');
  console.error('  hint: export TEMPO_PRIVATE_KEY=0x...');
  process.exit(1);
}

const PATHUSD = '0x20c0000000000000000000000000000000000000';
const PROVIDER = '0x002925FAFE98cfeB9fdBb7d6045ce318E4BD4b88';

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
];

async function main() {
  console.log();
  console.log('  Fee Sponsorship E2E Test');
  console.log('  ───────────────────────');
  console.log(`  Gateway: ${GATEWAY_URL}`);
  console.log();

  const account = Account.fromSecp256k1(PRIVATE_KEY);
  console.log(`  Payer: ${account.address}`);

  const client = createClient({
    account,
    chain: tempoModerato,
    transport: withFeePayer(
      http(),
      http(`${GATEWAY_URL}/paygate/sponsor`)
    ),
  })
    .extend(publicActions)
    .extend(walletActions)
    .extend(tempoActions());

  // Step 1: Get initial native balance
  const balanceBefore = await client.getBalance({ address: account.address });
  console.log(`  Balance before: ${balanceBefore}`);

  // Step 2: Send a sponsored TIP-20 transfer
  console.log('  Sending sponsored transferWithMemo...');
  const tx = await client.sendTransaction({
    to: PATHUSD,
    data: encodeFunctionData({
      abi: TIP20_ABI,
      functionName: 'transferWithMemo',
      args: [PROVIDER, 1000n, '0x' + '00'.repeat(32)],
    }),
    feePayer: true,
  });
  console.log(`  TX: ${tx}`);

  // Step 3: Wait for receipt
  const receipt = await client.waitForTransactionReceipt({ hash: tx });
  console.log(`  Status: ${receipt.status}`);

  // Step 4: Get final native balance
  const balanceAfter = await client.getBalance({ address: account.address });
  console.log(`  Balance after: ${balanceAfter}`);

  // Step 5: Assertions
  console.log();
  if (receipt.status !== 'success') {
    console.log('  Fee sponsorship E2E: FAIL (tx not successful)');
    process.exit(1);
  }

  if (balanceAfter < balanceBefore) {
    console.log('  Fee sponsorship E2E: FAIL (native balance decreased — gas was NOT sponsored)');
    console.log(`    Before: ${balanceBefore}`);
    console.log(`    After:  ${balanceAfter}`);
    process.exit(1);
  }

  console.log('  Fee sponsorship E2E: PASS');
  console.log(`    TX confirmed, native balance unchanged (gas sponsored by gateway)`);
  console.log();
}

main().catch((err) => {
  console.error('  Fee sponsorship E2E: FAIL');
  console.error(`    ${err.message}`);
  process.exit(1);
});
