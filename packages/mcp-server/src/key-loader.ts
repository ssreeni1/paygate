import { execSync } from 'node:child_process';

export function loadPrivateKey(): string {
  const cmd = process.env.PAYGATE_PRIVATE_KEY_CMD;
  if (cmd) {
    try {
      const result = execSync(cmd, {
        encoding: 'utf-8',
        timeout: 10_000,
        stdio: ['pipe', 'pipe', 'pipe'],
      }).trim();
      if (!result) {
        throw new Error('PAYGATE_PRIVATE_KEY_CMD returned empty output');
      }
      return normalizeKey(result);
    } catch (err) {
      throw new Error(
        `PAYGATE_PRIVATE_KEY_CMD failed: ${err instanceof Error ? err.message : String(err)}`
      );
    }
  }

  const key = process.env.PAYGATE_PRIVATE_KEY;
  if (key) {
    return normalizeKey(key);
  }

  throw new Error(
    'No private key configured. Set PAYGATE_PRIVATE_KEY or PAYGATE_PRIVATE_KEY_CMD.'
  );
}

function normalizeKey(key: string): string {
  const trimmed = key.trim();
  const withPrefix = trimmed.startsWith('0x') ? trimmed : `0x${trimmed}`;
  if (!/^0x[0-9a-fA-F]{64}$/.test(withPrefix)) {
    throw new Error('Invalid private key format: expected 32 bytes hex');
  }
  return withPrefix;
}
