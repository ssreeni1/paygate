import { describe, it, expect } from 'vitest';
import { classifyError, makeError, errorToMcpContent } from '../src/errors.js';

describe('classifyError', () => {
  it('classifies ECONNREFUSED as gateway_unreachable', () => {
    const err = classifyError(new Error('fetch failed: ECONNREFUSED'));
    expect(err.error).toBe('gateway_unreachable');
    expect(err.recoverable).toBe(true);
  });

  it('classifies balance errors as insufficient_balance', () => {
    const err = classifyError(new Error('insufficient balance for deposit'));
    expect(err.error).toBe('insufficient_balance');
    expect(err.recoverable).toBe(false);
  });

  it('classifies nonce errors as session_creation_failed', () => {
    const err = classifyError(new Error('Session creation failed: nonce expired'));
    expect(err.error).toBe('session_creation_failed');
    expect(err.recoverable).toBe(true);
  });

  it('classifies unknown errors as upstream_error', () => {
    const err = classifyError(new Error('something weird'));
    expect(err.error).toBe('upstream_error');
  });

  it('handles non-Error objects', () => {
    const err = classifyError('string error');
    expect(err.error).toBe('upstream_error');
    expect(err.message).toBe('string error');
  });
});

describe('errorToMcpContent', () => {
  it('wraps error as MCP isError content', () => {
    const err = makeError('invalid_input', 'bad input', false);
    const content = errorToMcpContent(err);
    expect(content.isError).toBe(true);
    const parsed = JSON.parse(content.content[0].text);
    expect(parsed.error).toBe('invalid_input');
    expect(parsed.recoverable).toBe(false);
  });
});
