import { describe, it, expect } from 'vitest';
import request from 'supertest';
import { app } from '../src/server.js';

describe('GET /v1/pricing', () => {
  it('returns the API catalog', async () => {
    const res = await request(app).get('/v1/pricing');
    expect(res.status).toBe(200);
    expect(res.body.apis).toHaveLength(4);

    const endpoints = res.body.apis.map((a: { endpoint: string }) => a.endpoint);
    expect(endpoints).toContain('POST /v1/search');
    expect(endpoints).toContain('POST /v1/scrape');
    expect(endpoints).toContain('POST /v1/image');
    expect(endpoints).toContain('POST /v1/summarize');
  });

  it('each API has required fields', async () => {
    const res = await request(app).get('/v1/pricing');
    for (const api of res.body.apis) {
      expect(api).toHaveProperty('endpoint');
      expect(api).toHaveProperty('price');
      expect(api).toHaveProperty('currency', 'USDC');
      expect(api).toHaveProperty('description');
      expect(api).toHaveProperty('example_request');
      expect(api).toHaveProperty('example_response_fields');
    }
  });
});
