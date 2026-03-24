import type { ActiveTrace, TraceInput } from '../types.js';
export declare function handleTrace(activeTraces: Map<string, ActiveTrace>): (input: TraceInput) => Promise<{
    content: Array<{
        type: "text";
        text: string;
    }>;
    isError?: boolean;
}>;
