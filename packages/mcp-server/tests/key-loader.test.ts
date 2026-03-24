import { describe, it, expect, afterEach } from 'vitest';
import { loadPrivateKey } from '../src/key-loader.js';

describe('loadPrivateKey', () => {
  const originalEnv = { ...process.env };

  afterEach(() => {
    process.env = { ...originalEnv };
  });

  it('loads key from PAYGATE_PRIVATE_KEY', () => {
    process.env.PAYGATE_PRIVATE_KEY = '0x' + 'ab'.repeat(32);
    delete process.env.PAYGATE_PRIVATE_KEY_CMD;
    expect(loadPrivateKey()).toBe('0x' + 'ab'.repeat(32));
  });

  it('adds 0x prefix if missing', () => {
    process.env.PAYGATE_PRIVATE_KEY = 'ab'.repeat(32);
    delete process.env.PAYGATE_PRIVATE_KEY_CMD;
    expect(loadPrivateKey()).toBe('0x' + 'ab'.repeat(32));
  });

  it('throws if neither env var is set', () => {
    delete process.env.PAYGATE_PRIVATE_KEY;
    delete process.env.PAYGATE_PRIVATE_KEY_CMD;
    expect(() => loadPrivateKey()).toThrow('No private key configured');
  });

  it('rejects invalid key format', () => {
    process.env.PAYGATE_PRIVATE_KEY = '0xinvalid';
    delete process.env.PAYGATE_PRIVATE_KEY_CMD;
    expect(() => loadPrivateKey()).toThrow('Invalid private key format');
  });
});

describe('loadPrivateKey with CMD', () => {
  const originalEnv = { ...process.env };

  afterEach(() => {
    process.env = { ...originalEnv };
  });

  it('loads key from PAYGATE_PRIVATE_KEY_CMD', () => {
    const key = '0x' + 'cd'.repeat(32);
    process.env.PAYGATE_PRIVATE_KEY_CMD = `echo ${key}`;
    delete process.env.PAYGATE_PRIVATE_KEY;
    expect(loadPrivateKey()).toBe(key);
  });

  it('CMD takes priority over PAYGATE_PRIVATE_KEY', () => {
    const cmdKey = '0x' + 'cd'.repeat(32);
    const envKey = '0x' + 'ab'.repeat(32);
    process.env.PAYGATE_PRIVATE_KEY_CMD = `echo ${cmdKey}`;
    process.env.PAYGATE_PRIVATE_KEY = envKey;
    expect(loadPrivateKey()).toBe(cmdKey);
  });

  it('throws if CMD returns empty', () => {
    process.env.PAYGATE_PRIVATE_KEY_CMD = 'echo';
    delete process.env.PAYGATE_PRIVATE_KEY;
    expect(() => loadPrivateKey()).toThrow('empty output');
  });
});
