import { keccak256, toBytes } from 'viem';

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
