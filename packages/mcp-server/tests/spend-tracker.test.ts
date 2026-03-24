import { describe, it, expect } from 'vitest';
import { SpendTracker, parseUsdcToBaseUnits, formatUsd } from '../src/spend-tracker.js';

describe('SpendTracker', () => {
  it('allows spending within daily limit', () => {
    const tracker = new SpendTracker(5_000_000, null);
    expect(tracker.checkLimit(1_000_000)).toBeNull();
  });

  it('rejects spending that exceeds daily limit', () => {
    const tracker = new SpendTracker(5_000_000, null);
    tracker.record_spend(4_500_000);
    const violation = tracker.checkLimit(1_000_000);
    expect(violation).not.toBeNull();
    expect(violation!.period).toBe('daily');
    expect(violation!.spent).toBe(4_500_000);
    expect(violation!.limit).toBe(5_000_000);
  });

  it('tracks cumulative spending', () => {
    const tracker = new SpendTracker(null, null);
    tracker.record_spend(100_000);
    tracker.record_spend(200_000);
    const record = tracker.getRecord();
    expect(record.totalSpentToday).toBe(300_000);
    expect(record.callCount).toBe(2);
  });

  it('returns Infinity for unlimited remaining', () => {
    const tracker = new SpendTracker(null, null);
    expect(tracker.remainingDaily()).toBe(Infinity);
    expect(tracker.remainingMonthly()).toBe(Infinity);
  });

  it('computes remaining correctly', () => {
    const tracker = new SpendTracker(5_000_000, 50_000_000);
    tracker.record_spend(2_000_000);
    expect(tracker.remainingDaily()).toBe(3_000_000);
    expect(tracker.remainingMonthly()).toBe(48_000_000);
  });
});

describe('parseUsdcToBaseUnits', () => {
  it('parses "5.00" to 5000000', () => {
    expect(parseUsdcToBaseUnits('5.00')).toBe(5_000_000);
  });

  it('returns null for undefined', () => {
    expect(parseUsdcToBaseUnits(undefined)).toBeNull();
  });

  it('returns null for negative', () => {
    expect(parseUsdcToBaseUnits('-1.00')).toBeNull();
  });
});

describe('formatUsd', () => {
  it('formats 1000 base units as $0.001000', () => {
    expect(formatUsd(1000)).toBe('$0.001000');
  });
});
