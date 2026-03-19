import {
  UpstreamRateLimitError,
  UpstreamTimeoutError,
  UpstreamServerError,
  UpstreamUnavailableError,
} from './errors.js';

export interface UpstreamOptions {
  timeout?: number;      // ms, default 30000
  retries?: number;      // default 0
  retryDelay?: number;   // ms, default 1000
}

export async function upstreamFetch(
  url: string,
  init: RequestInit,
  opts?: UpstreamOptions,
): Promise<Response> {
  const timeout = opts?.timeout ?? 30_000;
  const retries = opts?.retries ?? 0;
  const retryDelay = opts?.retryDelay ?? 1000;

  let lastError: Error | undefined;

  for (let attempt = 0; attempt <= retries; attempt++) {
    if (attempt > 0) {
      await new Promise(r => setTimeout(r, retryDelay));
    }

    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), timeout);

    try {
      const response = await fetch(url, {
        ...init,
        signal: controller.signal,
      });

      clearTimeout(timer);

      if (response.status === 429) {
        throw new UpstreamRateLimitError(url);
      }
      if (response.status >= 500) {
        throw new UpstreamServerError(url, response.status);
      }

      return response;
    } catch (err) {
      clearTimeout(timer);

      if (err instanceof UpstreamRateLimitError || err instanceof UpstreamServerError) {
        lastError = err;
        if (attempt < retries) continue;
        throw err;
      }

      if (err instanceof DOMException && err.name === 'AbortError') {
        lastError = new UpstreamTimeoutError(url);
        if (attempt < retries) continue;
        throw lastError;
      }

      lastError = new UpstreamUnavailableError(url, err instanceof Error ? err : undefined);
      if (attempt < retries) continue;
      throw lastError;
    }
  }

  throw lastError ?? new UpstreamUnavailableError(url);
}
