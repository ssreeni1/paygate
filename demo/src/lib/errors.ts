import type { Request, Response, NextFunction } from 'express';

export class ValidationError extends Error {
  statusCode = 400;
  constructor(message: string) {
    super(message);
    this.name = 'ValidationError';
  }
}

export class UpstreamRateLimitError extends Error {
  statusCode = 502;
  constructor(public upstream: string) {
    super(`upstream rate limited: ${upstream}`);
    this.name = 'UpstreamRateLimitError';
  }
}

export class UpstreamTimeoutError extends Error {
  statusCode = 504;
  constructor(public upstream: string) {
    super(`upstream timeout: ${upstream}`);
    this.name = 'UpstreamTimeoutError';
  }
}

export class UpstreamServerError extends Error {
  statusCode = 502;
  constructor(public upstream: string, public upstreamStatus: number) {
    super(`upstream error ${upstreamStatus}: ${upstream}`);
    this.name = 'UpstreamServerError';
  }
}

export class UpstreamUnavailableError extends Error {
  statusCode = 502;
  constructor(public upstream: string, cause?: Error) {
    super(`upstream unavailable: ${upstream}`);
    this.name = 'UpstreamUnavailableError';
    if (cause) this.cause = cause;
  }
}

type AppError = ValidationError | UpstreamRateLimitError | UpstreamTimeoutError | UpstreamServerError | UpstreamUnavailableError;

export function errorHandler(err: Error, _req: Request, res: Response, _next: NextFunction): void {
  const statusCode = (err as AppError).statusCode ?? 500;
  const name = err.name || 'InternalError';

  console.error(`[error] ${name}: ${err.message}`);

  res.status(statusCode).json({
    error: name,
    message: err.message,
  });
}
