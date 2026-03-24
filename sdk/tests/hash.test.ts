import { describe, it, expect } from 'vitest';
import { requestHash, paymentMemo, sessionMemo, hmacSha256 } from '../src/hash.js';
import vectors from '../../tests/fixtures/request_hash_vectors.json';

describe('requestHash cross-language parity', () => {
  for (const vector of vectors.request_hash_vectors) {
    it(`should match input encoding for: ${vector.description}`, () => {
      const hash = requestHash(vector.method, vector.path, vector.body);

      // Hash is a valid keccak256 output
      expect(hash).toMatch(/^0x[a-f0-9]{64}$/);

      // Verify input bytes match the expected hex from shared test vectors
      const encoder = new TextEncoder();
      const input = new Uint8Array([
        ...encoder.encode(vector.method),
        0x20, // space
        ...encoder.encode(vector.path),
        0x0a, // newline
        ...encoder.encode(vector.body),
      ]);
      const inputHex = Buffer.from(input).toString('hex');
      expect(inputHex).toBe(vector.input_hex);
    });

    it(`should match expected hash for: ${vector.description}`, () => {
      const hash = requestHash(vector.method, vector.path, vector.body);
      expect(hash).toBe(vector.expected_hash);
    });
  }

  for (const memoVector of vectors.memo_vectors) {
    it(`should match expected memo for: ${memoVector.description}`, () => {
      const rhVector = vectors.request_hash_vectors[memoVector.request_hash_vector_index];
      const rh = requestHash(rhVector.method, rhVector.path, rhVector.body);
      const memo = paymentMemo(memoVector.quote_id, rh);
      expect(memo).toBe(memoVector.expected_memo);
    });
  }

  it('should be deterministic', () => {
    const hash1 = requestHash('POST', '/v1/chat/completions', '{"model":"gpt-4"}');
    const hash2 = requestHash('POST', '/v1/chat/completions', '{"model":"gpt-4"}');
    expect(hash1).toBe(hash2);
  });

  it('should differ for different methods', () => {
    const getHash = requestHash('GET', '/v1/models', '');
    const postHash = requestHash('POST', '/v1/models', '');
    expect(getHash).not.toBe(postHash);
  });

  it('should differ for different paths', () => {
    const hash1 = requestHash('GET', '/v1/models', '');
    const hash2 = requestHash('GET', '/v1/chat', '');
    expect(hash1).not.toBe(hash2);
  });

  it('should differ for different bodies', () => {
    const hash1 = requestHash('POST', '/v1/chat', '{"a":1}');
    const hash2 = requestHash('POST', '/v1/chat', '{"a":2}');
    expect(hash1).not.toBe(hash2);
  });

  it('should handle Uint8Array body', () => {
    const bodyStr = '{"model":"gpt-4"}';
    const bodyBytes = new TextEncoder().encode(bodyStr);
    const hashStr = requestHash('POST', '/v1/chat', bodyStr);
    const hashBytes = requestHash('POST', '/v1/chat', bodyBytes);
    expect(hashStr).toBe(hashBytes);
  });
});

describe('sessionMemo', () => {
  it('should produce a valid 0x-prefixed 64-char hex string', () => {
    const memo = sessionMemo('nonce_abc123');
    expect(memo).toMatch(/^0x[a-f0-9]{64}$/);
  });

  it('should be deterministic (same input = same output)', () => {
    const memo1 = sessionMemo('nonce_abc123');
    const memo2 = sessionMemo('nonce_abc123');
    expect(memo1).toBe(memo2);
  });

  it('should produce different results for different nonces', () => {
    const memo1 = sessionMemo('nonce_abc123');
    const memo2 = sessionMemo('nonce_def456');
    expect(memo1).not.toBe(memo2);
  });
});

describe('hmacSha256', () => {
  it('should produce a valid hex string', () => {
    const sig = hmacSha256('aabbccdd', 'test message');
    expect(sig).toMatch(/^[a-f0-9]+$/);
  });

  it('should strip ssec_ prefix and produce the same result', () => {
    const sig1 = hmacSha256('aabbccdd', 'test message');
    const sig2 = hmacSha256('ssec_aabbccdd', 'test message');
    expect(sig1).toBe(sig2);
  });

  it('should produce different signatures for different messages', () => {
    const sig1 = hmacSha256('aabbccdd', 'message one');
    const sig2 = hmacSha256('aabbccdd', 'message two');
    expect(sig1).not.toBe(sig2);
  });

  it('should be deterministic (same input = same output)', () => {
    const sig1 = hmacSha256('aabbccdd', 'test message');
    const sig2 = hmacSha256('aabbccdd', 'test message');
    expect(sig1).toBe(sig2);
  });
});

describe('paymentMemo', () => {
  it('should produce deterministic output', () => {
    const rh = requestHash('POST', '/v1/chat/completions', '{"model":"gpt-4"}');
    const memo1 = paymentMemo('qt_abc123', rh);
    const memo2 = paymentMemo('qt_abc123', rh);
    expect(memo1).toBe(memo2);
  });

  it('should differ for different quotes', () => {
    const rh = requestHash('POST', '/v1/chat', '{}');
    const memo1 = paymentMemo('qt_abc', rh);
    const memo2 = paymentMemo('qt_def', rh);
    expect(memo1).not.toBe(memo2);
  });

  it('should differ for different request hashes', () => {
    const rh1 = requestHash('POST', '/v1/chat', '{"a":1}');
    const rh2 = requestHash('POST', '/v1/chat', '{"a":2}');
    const memo1 = paymentMemo('qt_same', rh1);
    const memo2 = paymentMemo('qt_same', rh2);
    expect(memo1).not.toBe(memo2);
  });

  it('should produce valid bytes32 hex', () => {
    const rh = requestHash('GET', '/test', '');
    const memo = paymentMemo('qt_test', rh);
    expect(memo).toMatch(/^0x[a-f0-9]{64}$/);
  });
});
