import { describe, it, expect } from 'vitest';
import { validateAddress, validatePrice, validateDirectory } from '../src/wizard.js';

describe('validateAddress', () => {
  it('accepts valid address', () => {
    expect(validateAddress('0x7F3a000000000000000000000000000000000001')).toBe(true);
  });

  it('rejects address without 0x prefix', () => {
    expect(validateAddress('7F3a000000000000000000000000000000000001')).toBe('Must start with 0x');
  });

  it('rejects short address', () => {
    expect(validateAddress('0x7F3a')).toBe('Must be exactly 42 characters');
  });

  it('rejects address with invalid hex', () => {
    expect(validateAddress('0xGGGG000000000000000000000000000000000001')).toBe(
      'Invalid hex characters',
    );
  });
});

describe('validatePrice', () => {
  it('accepts valid price', () => {
    expect(validatePrice('0.001')).toBe(true);
  });

  it('accepts zero price', () => {
    expect(validatePrice('0')).toBe(true);
  });

  it('accepts integer price', () => {
    expect(validatePrice('1')).toBe(true);
  });

  it('rejects negative price', () => {
    expect(validatePrice('-1')).toBe('Must be a non-negative number');
  });

  it('rejects non-numeric price', () => {
    expect(validatePrice('abc')).toBe('Must be a non-negative number');
  });

  it('rejects too many decimal places', () => {
    expect(validatePrice('0.0000001')).toBe('Maximum 6 decimal places');
  });

  it('accepts 6 decimal places', () => {
    expect(validatePrice('0.000001')).toBe(true);
  });
});

describe('validateDirectory', () => {
  it('accepts valid directory name', () => {
    expect(validateDirectory('my-api')).toBe(true);
  });

  it('rejects empty string', () => {
    expect(validateDirectory('')).toBe('Directory name is required');
  });

  it('rejects whitespace only', () => {
    expect(validateDirectory('   ')).toBe('Directory name is required');
  });
});
