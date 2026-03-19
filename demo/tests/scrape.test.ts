import { describe, it, expect, vi, beforeEach } from 'vitest';
import request from 'supertest';
import { app } from '../src/server.js';

// Mock playwright
vi.mock('playwright', () => {
  const mockPage = {
    goto: vi.fn().mockResolvedValue(undefined),
    content: vi.fn().mockResolvedValue('<html><head><title>Test Page</title></head><body><article><h1>Hello World</h1><p>Test content paragraph.</p></article></body></html>'),
    close: vi.fn().mockResolvedValue(undefined),
  };
  const mockContext = {
    newPage: vi.fn().mockResolvedValue(mockPage),
    close: vi.fn().mockResolvedValue(undefined),
  };
  const mockBrowser = {
    newContext: vi.fn().mockResolvedValue(mockContext),
    close: vi.fn().mockResolvedValue(undefined),
  };
  return {
    chromium: {
      launch: vi.fn().mockResolvedValue(mockBrowser),
    },
  };
});

beforeEach(() => {
  vi.clearAllMocks();
});

describe('POST /v1/scrape', () => {
  it('returns 400 for missing url', async () => {
    const res = await request(app)
      .post('/v1/scrape')
      .send({});

    expect(res.status).toBe(400);
    expect(res.body.error).toBe('ValidationError');
  });

  it('returns 400 for invalid url scheme', async () => {
    const res = await request(app)
      .post('/v1/scrape')
      .send({ url: 'ftp://example.com' });

    expect(res.status).toBe(400);
  });

  it('returns 500 when browser not initialized', async () => {
    // The browser isn't initialized in test mode (no initBrowser() call),
    // so this should return an error
    const res = await request(app)
      .post('/v1/scrape')
      .send({ url: 'https://example.com' });

    // Browser is null in test mode, should get 500
    expect(res.status).toBe(500);
  });
});
