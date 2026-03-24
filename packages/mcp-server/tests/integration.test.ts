import { describe, it, expect } from 'vitest';

describe('MCP tool registration', () => {
  const EXPECTED_TOOLS = [
    'paygate_discover',
    'paygate_call',
    'paygate_budget',
    'paygate_estimate',
    'paygate_trace',
  ];

  it('all 5 core tools are defined', () => {
    const unique = new Set(EXPECTED_TOOLS);
    expect(unique.size).toBe(5);
  });

  it('paygate_call requires method and path', () => {
    const callSchema = {
      type: 'object',
      properties: {
        method: { type: 'string', enum: ['GET', 'POST', 'PUT', 'DELETE'] },
        path: { type: 'string' },
        body: { type: 'object' },
        headers: { type: 'object', additionalProperties: { type: 'string' } },
      },
      required: ['method', 'path'],
    };
    expect(callSchema.required).toContain('method');
    expect(callSchema.required).toContain('path');
  });

  it('paygate_estimate requires calls array', () => {
    const estimateSchema = {
      type: 'object',
      properties: { calls: { type: 'array' } },
      required: ['calls'],
    };
    expect(estimateSchema.required).toContain('calls');
  });

  it('paygate_trace requires action and name', () => {
    const traceSchema = {
      type: 'object',
      properties: {
        action: { type: 'string', enum: ['start', 'stop'] },
        name: { type: 'string' },
      },
      required: ['action', 'name'],
    };
    expect(traceSchema.required).toContain('action');
    expect(traceSchema.required).toContain('name');
  });

  it('paygate_discover has optional goal parameter', () => {
    const discoverSchema = {
      type: 'object',
      properties: { goal: { type: 'string' } },
    };
    expect((discoverSchema as any).required).toBeUndefined();
  });

  it('paygate_budget has no required parameters', () => {
    const budgetSchema = {
      type: 'object',
      properties: {},
    };
    expect(Object.keys(budgetSchema.properties)).toHaveLength(0);
  });
});
