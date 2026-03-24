export class SpendTracker {
    record;
    dailyLimit;
    monthlyLimit;
    constructor(dailyLimit, monthlyLimit) {
        this.dailyLimit = dailyLimit;
        this.monthlyLimit = monthlyLimit;
        this.record = {
            totalSpentToday: 0,
            totalSpentThisMonth: 0,
            dayStartUtc: this.currentDayUtc(),
            monthStartUtc: this.currentMonthUtc(),
            callCount: 0,
        };
    }
    checkLimit(amount) {
        this.rolloverIfNeeded();
        if (this.dailyLimit !== null && this.record.totalSpentToday + amount > this.dailyLimit) {
            return { period: 'daily', spent: this.record.totalSpentToday, limit: this.dailyLimit };
        }
        if (this.monthlyLimit !== null && this.record.totalSpentThisMonth + amount > this.monthlyLimit) {
            return { period: 'monthly', spent: this.record.totalSpentThisMonth, limit: this.monthlyLimit };
        }
        return null;
    }
    record_spend(amount) {
        this.rolloverIfNeeded();
        this.record.totalSpentToday += amount;
        this.record.totalSpentThisMonth += amount;
        this.record.callCount += 1;
    }
    getRecord() {
        this.rolloverIfNeeded();
        return { ...this.record };
    }
    remainingDaily() {
        this.rolloverIfNeeded();
        if (this.dailyLimit === null)
            return Infinity;
        return Math.max(0, this.dailyLimit - this.record.totalSpentToday);
    }
    remainingMonthly() {
        this.rolloverIfNeeded();
        if (this.monthlyLimit === null)
            return Infinity;
        return Math.max(0, this.monthlyLimit - this.record.totalSpentThisMonth);
    }
    rolloverIfNeeded() {
        const currentDay = this.currentDayUtc();
        const currentMonth = this.currentMonthUtc();
        if (currentDay !== this.record.dayStartUtc) {
            this.record.totalSpentToday = 0;
            this.record.dayStartUtc = currentDay;
        }
        if (currentMonth !== this.record.monthStartUtc) {
            this.record.totalSpentThisMonth = 0;
            this.record.monthStartUtc = currentMonth;
        }
    }
    currentDayUtc() {
        return new Date().toISOString().slice(0, 10);
    }
    currentMonthUtc() {
        return new Date().toISOString().slice(0, 7);
    }
}
export function parseUsdcToBaseUnits(usdcStr) {
    if (!usdcStr)
        return null;
    const parsed = parseFloat(usdcStr);
    if (isNaN(parsed) || parsed < 0)
        return null;
    return Math.round(parsed * 1_000_000);
}
export function formatUsd(baseUnits) {
    return `$${(baseUnits / 1_000_000).toFixed(6)}`;
}
