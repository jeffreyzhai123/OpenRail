import { afterEach, describe, expect, test, vi } from 'vitest'
import { fixture } from '../test/fixtures'
import {
  ApiError,
  HttpClient,
  Latest,
  NetworkError,
  REQUEST_TIMEOUT_MS,
  TimeoutError,
  UNEXPECTED_RESPONSE,
  isAbortError,
} from './client'
import { DecodeError } from './decode'
import type { RunRequest } from './types'

/** What the fake server does on each successive call. */
type Step =
  | { status: number; body: unknown }
  | { status: number; text: string }
  | 'network error'
  | 'hang'

interface Call {
  url: string
  init: RequestInit
}

/** A `fetch` that follows a script, with no network. */
function fakeFetch(steps: Step[]): { fetch: typeof fetch; calls: Call[] } {
  const calls: Call[] = []
  const fakeFetch = async (
    input: RequestInfo | URL,
    init: RequestInit = {},
  ) => {
    calls.push({ url: String(input), init })
    const step = steps[calls.length - 1]
    if (step === undefined) throw new Error(`unexpected call ${calls.length}`)
    if (step === 'network error') throw new TypeError('Failed to fetch')
    if (step === 'hang') {
      return new Promise<Response>((_, reject) => {
        init.signal?.addEventListener('abort', () =>
          reject(init.signal?.reason),
        )
      })
    }
    const text = 'text' in step ? step.text : JSON.stringify(step.body)
    return { status: step.status, text: async () => text } as Response
  }
  return { fetch: fakeFetch as typeof fetch, calls }
}

function envelope(code: string): { error: { code: string; message: string } } {
  return { error: { code, message: `${code} happened` } }
}

const request: RunRequest = {
  scenario_id: 'charge-retry',
  seed: 0,
  handler: 'naive',
  fault_plan: null,
}

afterEach(() => {
  vi.useRealTimers()
})

describe('requests', () => {
  test('go to the base URL, with JSON bodies on POST', async () => {
    const server = fakeFetch([
      { status: 200, body: fixture('run-charge-retry-naive.json') },
      { status: 200, body: fixture('scenarios.json') },
    ])
    const client = new HttpClient({ fetch: server.fetch })

    await client.run(request)
    await client.scenarios()

    expect(server.calls[0].url).toBe('/api/run')
    expect(server.calls[0].init.method).toBe('POST')
    expect(server.calls[0].init.headers).toEqual({
      'content-type': 'application/json',
    })
    expect(JSON.parse(String(server.calls[0].init.body))).toEqual(request)
    expect(server.calls[1].url).toBe('/api/scenarios')
    expect(server.calls[1].init.method).toBe('GET')
    expect(server.calls[1].init.body).toBeUndefined()
  })

  test('replay puts the link in the path', async () => {
    const run = fixture('run-charge-retry-naive.json') as { replay: string }
    const server = fakeFetch([{ status: 200, body: fixture('replay.json') }])
    await new HttpClient({ fetch: server.fetch }).replay(run.replay)
    expect(server.calls[0].url).toBe(`/api/replay/${run.replay}`)
  })

  test('decode the response', async () => {
    const server = fakeFetch([{ status: 200, body: fixture('sweep.json') }])
    const sweep = await new HttpClient({ fetch: server.fetch }).sweep({
      scenario_id: 'charge-retry',
      seed_start: 0,
      count: 100,
    })
    expect(sweep).toEqual(fixture('sweep.json'))
  })
})

describe('retries', () => {
  test('a 503 is retried once, then succeeds', async () => {
    const server = fakeFetch([
      { status: 503, body: envelope('timeout') },
      { status: 200, body: fixture('run-charge-retry-naive.json') },
    ])
    await new HttpClient({ fetch: server.fetch }).run(request)
    expect(server.calls).toHaveLength(2)
  })

  test('a second 503 is final and keeps its code', async () => {
    const server = fakeFetch([
      { status: 503, body: envelope('timeout') },
      { status: 503, body: envelope('timeout') },
    ])
    const error = await new HttpClient({ fetch: server.fetch })
      .run(request)
      .catch((e) => e)
    expect(error).toBeInstanceOf(ApiError)
    expect(error).toMatchObject({ status: 503, code: 'timeout' })
    expect(server.calls).toHaveLength(2)
  })

  test('a 400 is not retried, and its envelope reaches the caller', async () => {
    const server = fakeFetch([{ status: 400, body: envelope('bad_request') }])
    const error = await new HttpClient({ fetch: server.fetch })
      .run(request)
      .catch((e) => e)
    expect(error).toMatchObject({
      status: 400,
      code: 'bad_request',
      message: 'bad_request happened',
    })
    expect(server.calls).toHaveLength(1)
  })

  test('a network error is retried once', async () => {
    const recovers = fakeFetch([
      'network error',
      { status: 200, body: fixture('run-charge-retry-naive.json') },
    ])
    await new HttpClient({ fetch: recovers.fetch }).run(request)
    expect(recovers.calls).toHaveLength(2)

    const stays = fakeFetch(['network error', 'network error'])
    const error = await new HttpClient({ fetch: stays.fetch })
      .run(request)
      .catch((e) => e)
    expect(error).toBeInstanceOf(NetworkError)
    expect(stays.calls).toHaveLength(2)
  })

  test("an error page that isn't the envelope is still an ApiError", async () => {
    const page = { status: 502, text: '<html>Bad Gateway</html>' }
    const server = fakeFetch([page, page])
    const error = await new HttpClient({ fetch: server.fetch })
      .run(request)
      .catch((e) => e)
    expect(error).toMatchObject({ status: 502, code: UNEXPECTED_RESPONSE })
  })
})

describe('bad responses', () => {
  test('malformed JSON is a DecodeError', async () => {
    const server = fakeFetch([{ status: 200, text: '{ not json' }])
    const error = await new HttpClient({ fetch: server.fetch })
      .run(request)
      .catch((e) => e)
    expect(error).toBeInstanceOf(DecodeError)
  })

  test('JSON of the wrong shape is a DecodeError', async () => {
    const server = fakeFetch([{ status: 200, body: { unexpected: true } }])
    const error = await new HttpClient({ fetch: server.fetch })
      .run(request)
      .catch((e) => e)
    expect(error).toBeInstanceOf(DecodeError)
  })
})

describe('timeouts and aborts', () => {
  test(`no answer within ${REQUEST_TIMEOUT_MS / 1000} s is a TimeoutError, not retried`, async () => {
    vi.useFakeTimers()
    const server = fakeFetch(['hang'])
    const outcome = new HttpClient({ fetch: server.fetch })
      .run(request)
      .catch((e) => e)

    await vi.advanceTimersByTimeAsync(REQUEST_TIMEOUT_MS)

    expect(await outcome).toBeInstanceOf(TimeoutError)
    expect(server.calls).toHaveLength(1)
  })

  test("the caller's abort rejects with an AbortError", async () => {
    const server = fakeFetch(['hang'])
    const controller = new AbortController()
    const outcome = new HttpClient({ fetch: server.fetch })
      .run(request, { signal: controller.signal })
      .catch((e) => e)

    controller.abort()

    expect(isAbortError(await outcome)).toBe(true)
    expect(server.calls).toHaveLength(1)
  })

  test('a newer call aborts the superseded one, so only the latest resolves', async () => {
    const server = fakeFetch([
      'hang',
      { status: 200, body: fixture('run-charge-retry-naive.json') },
    ])
    const client = new HttpClient({ fetch: server.fetch })
    const runs = new Latest()

    const first = runs
      .start((signal) => client.run(request, { signal }))
      .catch((e) => e)
    const second = runs.start((signal) => client.run(request, { signal }))

    expect(isAbortError(await first)).toBe(true)
    expect(await second).toEqual(fixture('run-charge-retry-naive.json'))
  })
})
