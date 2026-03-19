import { ValidationError } from './errors.js';

export function requireString(value: unknown, field: string): string {
  if (typeof value !== 'string' || value.trim().length === 0) {
    throw new ValidationError(`${field} is required and must be a non-empty string`);
  }
  return value.trim();
}

export function requireUrl(value: unknown, field: string): string {
  const s = requireString(value, field);
  if (!s.startsWith('http://') && !s.startsWith('https://')) {
    throw new ValidationError(`${field} must start with http:// or https://`);
  }
  return s;
}

export function optionalNumber(
  value: unknown,
  field: string,
  defaultVal: number,
  min: number,
  max: number,
): number {
  if (value === undefined || value === null) return defaultVal;
  const n = typeof value === 'number' ? value : Number(value);
  if (isNaN(n) || !Number.isInteger(n)) {
    throw new ValidationError(`${field} must be an integer`);
  }
  if (n < min || n > max) {
    throw new ValidationError(`${field} must be between ${min} and ${max}`);
  }
  return n;
}

export function requireStringMaxLength(value: unknown, field: string, maxLength: number): string {
  const s = requireString(value, field);
  if (s.length > maxLength) {
    throw new ValidationError(`${field} must be at most ${maxLength} characters`);
  }
  return s;
}
