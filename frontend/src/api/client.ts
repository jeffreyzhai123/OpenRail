// The network boundary (specs/v1-frontend-tasks.md, "Principles"): timeouts,
// one retry for failures a second try can fix, validation of every response,
// and a way to abort superseded requests. Every sim-api endpoint is a pure
// function of its request, so retrying a POST is safe.

import {
  DecodeError,
  decodeErrorEnvelope,
  decodeRunResponse,
  decodeScenarios,
  decodeShrinkResponse,
  decodeSweepResponse,
} from './decode'
import type {
  RunRequest,
  RunResponse,
  ScenarioSummary,
  ShrinkRequest,
  ShrinkResponse,
  SweepRequest,
  SweepResponse,
} from './types'

/** Longer than sim-api's 10 s compute limit, so the server answers
 * `timeout` before the client gives up. */
export const REQUEST_TIMEOUT_MS = 15_000

/** Gateway failures a second try can fix. Every other status is final. */
const RETRYABLE_STATUSES = new Set([502, 503, 504])

/** An error response that isn't sim-api's envelope, such as a proxy's 502 page. */
export const UNEXPECTED_RESPONSE = 'unexpected_response'

export interface CallOptions {
  signal?: AbortSignal
}

export interface SimClient {
  scenarios(options?: CallOptions): Promise<ScenarioSummary[]>
  run(request: RunRequest, options?: CallOptions): Promise<RunResponse>
  replay(encoded: string, options?: CallOptions): Promise<RunResponse>
  shrink(request: ShrinkRequest, options?: CallOptions): Promise<ShrinkResponse>
  sweep(request: SweepRequest, options?: CallOptions): Promise<SweepResponse>
}

/** An error response from sim-api. Match on `code`, never on `message`. */
export class ApiError extends Error {
  readonly status: number
  readonly code: string

  constructor(status: number, code: string, message: string) {
    super(message)
    this.name = 'ApiError'
    this.status = status
    this.code = code
  }
}

export class TimeoutError extends Error {
  constructor(timeoutMs: number) {
    super(`sim-api didn't answer within ${timeoutMs / 1000} s`)
    this.name = 'TimeoutError'
  }
}

/** The request never got a response, even after its retry. */
export class NetworkError extends Error {
  constructor(cause: unknown) {
    super("couldn't reach sim-api", { cause })
    this.name = 'NetworkError'
  }
}

/** True for the rejection of a call the caller aborted on purpose. Checks
 * the name, not `instanceof DOMException`: the abort reason can come from
 * another realm (an iframe, or jsdom in tests). */
export function isAbortError(error: unknown): boolean {
  return (
    typeof error === 'object' &&
    error !== null &&
    'name' in error &&
    error.name === 'AbortError'
  )
}

export interface HttpClientOptions {
  /** Defaults to `/api`, which the Vite dev proxy forwards to sim-api. */
  baseUrl?: string
  fetch?: typeof fetch
  timeoutMs?: number
}

/** One attempt's outcome, read in full under the timeout. */
interface Reply {
  status: number
  body: string
}

export class HttpClient implements SimClient {
  private readonly baseUrl: string
  private readonly fetchFn: typeof fetch
  private readonly timeoutMs: number

  constructor(options: HttpClientOptions = {}) {
    this.baseUrl = options.baseUrl ?? '/api'
    this.fetchFn = options.fetch ?? ((input, init) => fetch(input, init))
    this.timeoutMs = options.timeoutMs ?? REQUEST_TIMEOUT_MS
  }

  scenarios(options?: CallOptions): Promise<ScenarioSummary[]> {
    return this.call('GET', '/scenarios', undefined, decodeScenarios, options)
  }

  run(request: RunRequest, options?: CallOptions): Promise<RunResponse> {
    return this.call('POST', '/run', request, decodeRunResponse, options)
  }

  replay(encoded: string, options?: CallOptions): Promise<RunResponse> {
    const path = `/replay/${encodeURIComponent(encoded)}`
    return this.call('GET', path, undefined, decodeRunResponse, options)
  }

  shrink(
    request: ShrinkRequest,
    options?: CallOptions,
  ): Promise<ShrinkResponse> {
    return this.call('POST', '/shrink', request, decodeShrinkResponse, options)
  }

  sweep(request: SweepRequest, options?: CallOptions): Promise<SweepResponse> {
    return this.call('POST', '/sweep', request, decodeSweepResponse, options)
  }

  private async call<T>(
    method: 'GET' | 'POST',
    path: string,
    body: unknown,
    decode: (value: unknown) => T,
    options: CallOptions = {},
  ): Promise<T> {
    const reply = await this.attemptWithRetry(
      method,
      path,
      body,
      options.signal,
    )
    return read(reply, decode)
  }

  /** A second and final attempt only after a network error or a gateway failure. */
  private async attemptWithRetry(
    method: 'GET' | 'POST',
    path: string,
    body: unknown,
    signal: AbortSignal | undefined,
  ): Promise<Reply> {
    try {
      const reply = await this.attempt(method, path, body, signal)
      if (!RETRYABLE_STATUSES.has(reply.status)) {
        return reply
      }
    } catch (error) {
      if (!(error instanceof NetworkError)) {
        throw error
      }
    }
    return this.attempt(method, path, body, signal)
  }

  /** Fetches and reads the body, aborted by the caller's signal or the timeout. */
  private async attempt(
    method: 'GET' | 'POST',
    path: string,
    body: unknown,
    signal: AbortSignal | undefined,
  ): Promise<Reply> {
    const controller = new AbortController()
    let timedOut = false
    const timer = setTimeout(() => {
      timedOut = true
      controller.abort()
    }, this.timeoutMs)
    const forwardAbort = () => controller.abort(signal?.reason)
    if (signal?.aborted) {
      forwardAbort()
    }
    signal?.addEventListener('abort', forwardAbort, { once: true })

    try {
      const response = await this.fetchFn(this.baseUrl + path, {
        method,
        headers:
          body === undefined
            ? undefined
            : { 'content-type': 'application/json' },
        body: body === undefined ? undefined : JSON.stringify(body),
        signal: controller.signal,
      })
      return { status: response.status, body: await response.text() }
    } catch (error) {
      if (timedOut) {
        throw new TimeoutError(this.timeoutMs)
      }
      if (signal?.aborted) {
        throw error
      }
      throw new NetworkError(error)
    } finally {
      clearTimeout(timer)
      signal?.removeEventListener('abort', forwardAbort)
    }
  }
}

const NOT_JSON = Symbol('not JSON')

function parseJson(text: string): unknown {
  try {
    return JSON.parse(text)
  } catch {
    return NOT_JSON
  }
}

function read<T>(reply: Reply, decode: (value: unknown) => T): T {
  const json = parseJson(reply.body)
  if (reply.status < 200 || reply.status >= 300) {
    throw apiError(reply.status, json)
  }
  if (json === NOT_JSON) {
    throw new DecodeError('response', 'JSON')
  }
  return decode(json)
}

function apiError(status: number, json: unknown): ApiError {
  const envelope =
    json === NOT_JSON ? null : decodeOrNull(decodeErrorEnvelope, json)
  return envelope
    ? new ApiError(status, envelope.error.code, envelope.error.message)
    : new ApiError(status, UNEXPECTED_RESPONSE, `HTTP ${status}`)
}

function decodeOrNull<T>(
  decode: (value: unknown) => T,
  value: unknown,
): T | null {
  try {
    return decode(value)
  } catch (error) {
    if (error instanceof DecodeError) {
      return null
    }
    throw error
  }
}

/** Starting a call aborts the previous one, if it's still running, so a slow
 * old response can't overwrite a newer one. */
export class Latest {
  private controller: AbortController | null = null

  start<T>(call: (signal: AbortSignal) => Promise<T>): Promise<T> {
    this.controller?.abort()
    const controller = new AbortController()
    this.controller = controller
    return call(controller.signal)
  }
}
