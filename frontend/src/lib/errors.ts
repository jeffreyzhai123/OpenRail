// What the error banner says. sim-api's codes are matched exactly, never its
// message text (specs/v1-frontend-tasks.md, "What the contract implies").

import {
  ApiError,
  NetworkError,
  TimeoutError,
  UNEXPECTED_RESPONSE,
} from '../api/client'
import { DecodeError } from '../api/decode'
import { MAX_PLAN_FAULTS } from '../api/types'
import { BalanceMismatchError } from './balances'

/** The codes the UI explains in its own words. */
const API_MESSAGES: Record<string, string> = {
  plan_too_long: `The fault plan is too long: sim-api accepts at most ${MAX_PLAN_FAULTS} faults.`,
  too_many_seeds:
    'That sweep covers too many seeds: sim-api sweeps at most 1,000.',
  does_not_fail:
    "That plan doesn't fail the invariant, so there's nothing to shrink.",
  unknown_invariant: "sim-api doesn't know that invariant.",
  invalid_replay: "This share link is damaged, so it can't be replayed.",
  unsupported_encoding_version:
    "This share link uses an encoding version this server doesn't read.",
  timeout: 'The simulation took too long. Try again in a moment.',
}

const UNREACHABLE =
  "Couldn't reach sim-api. In development, start it with `cargo run -p sim-api`."

/** Statuses a proxy sends for a backend it can't reach. */
const GATEWAY_STATUSES = new Set([502, 503, 504])

export function errorMessage(error: unknown): string {
  if (error instanceof ApiError) {
    // sim-api's own errors carry its envelope. A bare gateway status comes
    // from a proxy in front of it, such as the Vite dev proxy when sim-api
    // isn't running.
    if (
      error.code === UNEXPECTED_RESPONSE &&
      GATEWAY_STATUSES.has(error.status)
    ) {
      return UNREACHABLE
    }
    return API_MESSAGES[error.code] ?? error.message
  }
  if (error instanceof TimeoutError) {
    return "sim-api didn't answer in time."
  }
  if (error instanceof NetworkError) {
    return UNREACHABLE
  }
  if (error instanceof DecodeError) {
    return `sim-api sent a response this app doesn't understand (at ${error.path}).`
  }
  if (error instanceof BalanceMismatchError) {
    return `This run's journal doesn't add up, so its balances aren't shown: ${error.message}.`
  }
  return 'Something went wrong.'
}
