import { keccak256, toBytes } from 'viem';
import { createHmac } from 'crypto';

/**
 * Compute requestHash = keccak256(method + " " + path + "\n" + body).
 *
 * This MUST produce identical output to paygate-common/src/hash.rs::request_hash().
 * Both use UTF-8 encoding of the concatenated string, then keccak256.
 */
export function requestHash(method: string, path: string, body: string | Uint8Array): `0x${string}` {
  const encoder = new TextEncoder();
  const methodBytes = encoder.encode(method);
  const spaceBytes = new Uint8Array([0x20]);
  const pathBytes = encoder.encode(path);
  const newlineBytes = new Uint8Array([0x0a]);
  const bodyBytes = typeof body === 'string' ? encoder.encode(body) : body;

  const input = new Uint8Array(
    methodBytes.length + 1 + pathBytes.length + 1 + bodyBytes.length
  );
  let offset = 0;
  input.set(methodBytes, offset); offset += methodBytes.length;
  input.set(spaceBytes, offset); offset += 1;
  input.set(pathBytes, offset); offset += pathBytes.length;
  input.set(newlineBytes, offset); offset += 1;
  input.set(bodyBytes, offset);

  return keccak256(input);
}

/**
 * Compute payment memo = keccak256("paygate" + quoteId + requestHash).
 *
 * requestHash is the raw 32-byte hash (not hex-encoded).
 * This MUST produce identical output to paygate-common/src/hash.rs::payment_memo().
 */
export function paymentMemo(quoteId: string, reqHash: `0x${string}`): `0x${string}` {
  const encoder = new TextEncoder();
  const prefixBytes = encoder.encode('paygate');
  const quoteBytes = encoder.encode(quoteId);
  const hashBytes = toBytes(reqHash); // 32 bytes from hex

  const input = new Uint8Array(prefixBytes.length + quoteBytes.length + hashBytes.length);
  let offset = 0;
  input.set(prefixBytes, offset); offset += prefixBytes.length;
  input.set(quoteBytes, offset); offset += quoteBytes.length;
  input.set(hashBytes, offset);

  return keccak256(input);
}

/**
 * Compute session memo = keccak256("paygate-session" + nonce).
 * Used when creating a deposit for a pay-as-you-go session.
 */
export function sessionMemo(nonce: string): `0x${string}` {
  const input = new TextEncoder().encode('paygate-session' + nonce);
  return keccak256(input);
}

/**
 * Compute HMAC-SHA256 for session authentication.
 * The secret is expected in hex format, optionally prefixed with "ssec_".
 */
export function hmacSha256(secret: string, message: string): string {
  const rawSecret = secret.startsWith('ssec_') ? secret.slice(5) : secret;
  return createHmac('sha256', Buffer.from(rawSecret, 'hex'))
    .update(message)
    .digest('hex');
}
