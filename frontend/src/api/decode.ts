// Hand-written guards for every sim-api response. Anything that doesn't match
// the contract is rejected here, at the boundary, so the rest of the app can
// trust its types. JS has no i64, so every number must be a safe integer.

import {
  MAX_SEED,
  type AchEvent,
  type AchReturnCode,
  type CardEvent,
  type ErrorEnvelope,
  type EventKind,
  type FaultOp,
  type Handler,
  type InvariantResult,
  type JournalEntry,
  type Posting,
  type RunResponse,
  type ScenarioSummary,
  type ShrinkResponse,
  type SimEvent,
  type SweepResponse,
} from './types'

/** A response that doesn't match the contract, with where it went wrong. */
export class DecodeError extends Error {
  readonly path: string

  constructor(path: string, expected: string) {
    super(`${path}: expected ${expected}`)
    this.name = 'DecodeError'
    this.path = path
  }
}

type Fields = Record<string, unknown>

function fail(path: string, expected: string): never {
  throw new DecodeError(path, expected)
}

function object(value: unknown, path: string): Fields {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) {
    return fail(path, 'an object')
  }
  return value as Fields
}

function array(value: unknown, path: string): unknown[] {
  return Array.isArray(value) ? value : fail(path, 'an array')
}

function field(fields: Fields, key: string, path: string): unknown {
  return key in fields ? fields[key] : fail(`${path}.${key}`, 'a value')
}

function string(fields: Fields, key: string, path: string): string {
  const value = field(fields, key, path)
  return typeof value === 'string' ? value : fail(`${path}.${key}`, 'a string')
}

function boolean(fields: Fields, key: string, path: string): boolean {
  const value = field(fields, key, path)
  return typeof value === 'boolean'
    ? value
    : fail(`${path}.${key}`, 'a boolean')
}

function safeInteger(value: unknown, path: string): number {
  return typeof value === 'number' && Number.isSafeInteger(value)
    ? value
    : fail(path, 'a safe integer')
}

/** Money and other signed integers. */
function integer(fields: Fields, key: string, path: string): number {
  return safeInteger(field(fields, key, path), `${path}.${key}`)
}

/** Ids, times, counts and other unsigned integers. */
function count(fields: Fields, key: string, path: string): number {
  const value = integer(fields, key, path)
  return value >= 0 ? value : fail(`${path}.${key}`, 'a non-negative integer')
}

function seed(fields: Fields, key: string, path: string): number {
  const value = field(fields, key, path)
  return typeof value === 'number' &&
    Number.isInteger(value) &&
    value >= 0 &&
    value <= MAX_SEED
    ? value
    : fail(`${path}.${key}`, `an integer in 0..=${MAX_SEED}`)
}

function list<T>(
  fields: Fields,
  key: string,
  path: string,
  decode: (value: unknown, path: string) => T,
): T[] {
  return array(field(fields, key, path), `${path}.${key}`).map((item, index) =>
    decode(item, `${path}.${key}[${index}]`),
  )
}

/** serde's externally tagged enums: `{ "Variant": body }`. */
function tagged(value: unknown, path: string): [string, unknown] {
  const fields = object(value, path)
  const keys = Object.keys(fields)
  return keys.length === 1
    ? [keys[0], fields[keys[0]]]
    : fail(path, 'an object with exactly one variant')
}

function centsRecord(value: unknown, path: string): Record<string, number> {
  const fields = object(value, path)
  const record: Record<string, number> = {}
  for (const key of Object.keys(fields)) {
    record[key] = safeInteger(fields[key], `${path}.${key}`)
  }
  return record
}

function handler(fields: Fields, key: string, path: string): Handler {
  const value = field(fields, key, path)
  return value === 'naive' || value === 'hardened'
    ? value
    : fail(`${path}.${key}`, '"naive" or "hardened"')
}

function chargeAmount(
  body: unknown,
  path: string,
): { charge_id: number; amount: number } {
  const fields = object(body, path)
  return {
    charge_id: count(fields, 'charge_id', path),
    amount: integer(fields, 'amount', path),
  }
}

function cardEvent(value: unknown, path: string): CardEvent {
  const [tag, body] = tagged(value, path)
  const at = `${path}.${tag}`
  switch (tag) {
    case 'Authorized':
      return { Authorized: chargeAmount(body, at) }
    case 'Captured':
      return { Captured: chargeAmount(body, at) }
    case 'Refunded':
      return { Refunded: chargeAmount(body, at) }
    default:
      return fail(path, `a card event, not "${tag}"`)
  }
}

function returnCode(fields: Fields, key: string, path: string): AchReturnCode {
  const value = field(fields, key, path)
  if (
    value === 'R01' ||
    value === 'R02' ||
    value === 'R03' ||
    value === 'R04'
  ) {
    return value
  }
  const [tag, body] = tagged(value, `${path}.${key}`)
  return tag === 'Other' && typeof body === 'string'
    ? { Other: body }
    : fail(`${path}.${key}`, 'an ACH return code')
}

function entryId(body: unknown, path: string): { entry_id: number } {
  return { entry_id: count(object(body, path), 'entry_id', path) }
}

function achEvent(value: unknown, path: string): AchEvent {
  const [tag, body] = tagged(value, path)
  const at = `${path}.${tag}`
  switch (tag) {
    case 'Initiated': {
      const fields = object(body, at)
      return {
        Initiated: {
          entry_id: count(fields, 'entry_id', at),
          amount: integer(fields, 'amount', at),
        },
      }
    }
    case 'Batched':
      return { Batched: entryId(body, at) }
    case 'Settled':
      return { Settled: entryId(body, at) }
    case 'Returned': {
      const fields = object(body, at)
      return {
        Returned: {
          entry_id: count(fields, 'entry_id', at),
          code: returnCode(fields, 'code', at),
          amount: integer(fields, 'amount', at),
        },
      }
    }
    default:
      return fail(path, `an ACH event, not "${tag}"`)
  }
}

function eventKind(value: unknown, path: string): EventKind {
  const [tag, body] = tagged(value, path)
  switch (tag) {
    case 'Card':
      return { Card: cardEvent(body, `${path}.Card`) }
    case 'Ach':
      return { Ach: achEvent(body, `${path}.Ach`) }
    default:
      return fail(path, `an event kind, not "${tag}"`)
  }
}

function simEvent(value: unknown, path: string): SimEvent {
  const fields = object(value, path)
  return {
    id: count(fields, 'id', path),
    time: count(fields, 'time', path),
    seq: count(fields, 'seq', path),
    kind: eventKind(field(fields, 'kind', path), `${path}.kind`),
  }
}

function eventIdOnly(body: unknown, path: string): { event_id: number } {
  return { event_id: count(object(body, path), 'event_id', path) }
}

export function decodeFaultOp(value: unknown, path = 'fault'): FaultOp {
  const [tag, body] = tagged(value, path)
  const at = `${path}.${tag}`
  switch (tag) {
    case 'Duplicate':
      return { Duplicate: eventIdOnly(body, at) }
    case 'Drop':
      return { Drop: eventIdOnly(body, at) }
    case 'Reorder': {
      const fields = object(body, at)
      return {
        Reorder: {
          event_id: count(fields, 'event_id', at),
          window: count(fields, 'window', at),
        },
      }
    }
    case 'Delay': {
      const fields = object(body, at)
      return {
        Delay: {
          event_id: count(fields, 'event_id', at),
          by: count(fields, 'by', at),
        },
      }
    }
    case 'CrashRestart':
      return { CrashRestart: { at: count(object(body, at), 'at', at) } }
    default:
      return fail(path, `a fault op, not "${tag}"`)
  }
}

function posting(value: unknown, path: string): Posting {
  const fields = object(value, path)
  return {
    account: string(fields, 'account', path),
    delta: integer(fields, 'delta', path),
  }
}

function journalEntry(value: unknown, path: string): JournalEntry {
  const fields = object(value, path)
  const kind = field(fields, 'kind', path)
  return {
    source: count(fields, 'source', path),
    intent: string(fields, 'intent', path),
    kind:
      kind === 'Capture' || kind === 'Refund'
        ? kind
        : fail(`${path}.kind`, '"Capture" or "Refund"'),
    postings: list(fields, 'postings', path, posting),
  }
}

function invariantResult(value: unknown, path: string): InvariantResult {
  const fields = object(value, path)
  const message = field(fields, 'message', path)
  return {
    name: string(fields, 'name', path),
    passed: boolean(fields, 'passed', path),
    message:
      message === null || typeof message === 'string'
        ? message
        : fail(`${path}.message`, 'a string or null'),
  }
}

export function decodeRunResponse(value: unknown, path = 'run'): RunResponse {
  const fields = object(value, path)
  const ledger = object(field(fields, 'ledger', path), `${path}.ledger`)
  return {
    scenario_id: string(fields, 'scenario_id', path),
    seed: seed(fields, 'seed', path),
    handler: handler(fields, 'handler', path),
    fault_plan: list(fields, 'fault_plan', path, decodeFaultOp),
    trace: list(fields, 'trace', path, simEvent),
    opening: centsRecord(field(fields, 'opening', path), `${path}.opening`),
    journal: list(fields, 'journal', path, journalEntry),
    ledger: {
      accounts: centsRecord(
        field(ledger, 'accounts', `${path}.ledger`),
        `${path}.ledger.accounts`,
      ),
    },
    invariants: list(fields, 'invariants', path, invariantResult),
    trace_hash: string(fields, 'trace_hash', path),
    replay: string(fields, 'replay', path),
  }
}

function scenarioSummary(value: unknown, path: string): ScenarioSummary {
  const fields = object(value, path)
  return {
    id: string(fields, 'id', path),
    name: string(fields, 'name', path),
    description: string(fields, 'description', path),
    accounts: array(field(fields, 'accounts', path), `${path}.accounts`).map(
      (account, index) =>
        typeof account === 'string'
          ? account
          : fail(`${path}.accounts[${index}]`, 'a string'),
    ),
    workload: list(fields, 'workload', path, simEvent),
    story_plan: list(fields, 'story_plan', path, decodeFaultOp),
  }
}

export function decodeScenarios(value: unknown): ScenarioSummary[] {
  return array(value, 'scenarios').map((item, index) =>
    scenarioSummary(item, `scenarios[${index}]`),
  )
}

export function decodeShrinkResponse(value: unknown): ShrinkResponse {
  const path = 'shrink'
  const fields = object(value, path)
  return {
    original: list(fields, 'original', path, decodeFaultOp),
    shrunk: list(fields, 'shrunk', path, decodeFaultOp),
    invariant: string(fields, 'invariant', path),
    candidates_tried: count(fields, 'candidates_tried', path),
    run: decodeRunResponse(field(fields, 'run', path), `${path}.run`),
  }
}

export function decodeSweepResponse(value: unknown): SweepResponse {
  const path = 'sweep'
  const fields = object(value, path)
  const failures = (key: string) => {
    const handlerFields = object(field(fields, key, path), `${path}.${key}`)
    return { failed: count(handlerFields, 'failed', `${path}.${key}`) }
  }
  return {
    count: count(fields, 'count', path),
    naive: failures('naive'),
    hardened: failures('hardened'),
  }
}

export function decodeErrorEnvelope(value: unknown): ErrorEnvelope {
  const path = 'error'
  const error = object(field(object(value, path), 'error', path), path)
  return {
    error: {
      code: string(error, 'code', path),
      message: string(error, 'message', path),
    },
  }
}
