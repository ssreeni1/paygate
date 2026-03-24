import type { SpendRecord } from './types.js';
export declare class SpendTracker {
    private record;
    private dailyLimit;
    private monthlyLimit;
    constructor(dailyLimit: number | null, monthlyLimit: number | null);
    checkLimit(amount: number): {
        period: 'daily' | 'monthly';
        spent: number;
        limit: number;
    } | null;
    record_spend(amount: number): void;
    getRecord(): Readonly<SpendRecord>;
    remainingDaily(): number;
    remainingMonthly(): number;
    private rolloverIfNeeded;
    private currentDayUtc;
    private currentMonthUtc;
}
export declare function parseUsdcToBaseUnits(usdcStr: string | undefined): number | null;
export declare function formatUsd(baseUnits: number): string;
