// Pure helpers for the fault plan editor: describe, add, remove and replace
// faults, plus the shape checks that catch what sim-api would reject anyway.
// Plans are never mutated; every change returns a new plan.

import {
  DUPLICATE_REDELIVERY_MS,
  MAX_PLAN_FAULTS,
  type EventId,
  type FaultOp,
  type SimEvent,
} from '../api/types'
import { formatDuration } from './time'
import { parseWholeNumber } from './wholeNumber'

export type FaultKind =
  'Duplicate' | 'Reorder' | 'Delay' | 'Drop' | 'CrashRestart'

export function faultKind(op: FaultOp): FaultKind {
  if ('Duplicate' in op) return 'Duplicate'
  if ('Reorder' in op) return 'Reorder'
  if ('Delay' in op) return 'Delay'
  if ('Drop' in op) return 'Drop'
  return 'CrashRestart'
}

/** The event an op targets, or `null` for a crash-restart, which targets a time. */
export function faultTarget(op: FaultOp): EventId | null {
  if ('Duplicate' in op) return op.Duplicate.event_id
  if ('Reorder' in op) return op.Reorder.event_id
  if ('Delay' in op) return op.Delay.event_id
  if ('Drop' in op) return op.Drop.event_id
  return null
}

/** One line for the plan list, e.g. "Delay event 3 by 45 s". */
export function describeFault(op: FaultOp): string {
  if ('Duplicate' in op) {
    const later = formatDuration(DUPLICATE_REDELIVERY_MS)
    return `Redeliver event ${op.Duplicate.event_id} ${later} later`
  }
  if ('Reorder' in op) {
    const { event_id, window } = op.Reorder
    return `Reverse the ${window} deliveries starting at event ${event_id}`
  }
  if ('Delay' in op) {
    return `Delay event ${op.Delay.event_id} by ${formatDuration(op.Delay.by)}`
  }
  if ('Drop' in op) return `Drop event ${op.Drop.event_id}`
  return `Crash and restart the handler at ${formatDuration(op.CrashRestart.at)}`
}

/** sim-api rejects plans over MAX_PLAN_FAULTS, so the editor stops there. */
export function canAddFault(plan: readonly FaultOp[]): boolean {
  return plan.length < MAX_PLAN_FAULTS
}

export function addFault(plan: readonly FaultOp[], op: FaultOp): FaultOp[] {
  if (!canAddFault(plan)) {
    throw new RangeError(`a plan holds at most ${MAX_PLAN_FAULTS} faults`)
  }
  return [...plan, op]
}

export function removeFault(
  plan: readonly FaultOp[],
  index: number,
): FaultOp[] {
  checkIndex(plan, index)
  return plan.filter((_, at) => at !== index)
}

export function replaceFault(
  plan: readonly FaultOp[],
  index: number,
  op: FaultOp,
): FaultOp[] {
  checkIndex(plan, index)
  return plan.map((existing, at) => (at === index ? op : existing))
}

function checkIndex(plan: readonly FaultOp[], index: number): void {
  if (!Number.isInteger(index) || index < 0 || index >= plan.length) {
    throw new RangeError(`no fault at index ${index}`)
  }
}

const NOT_WHOLE = 'every number must be a whole number, 0 or more'

/** What's wrong with an op the editor built, or `null` if sim-api will accept
 * it: its numbers must be non-negative whole numbers, and its target must be
 * one of the scenario's events. */
export function faultProblem(
  op: FaultOp,
  workload: readonly SimEvent[],
): string | null {
  const numbers = fieldsOf(op)
  if (!numbers.every((value) => Number.isSafeInteger(value) && value >= 0)) {
    return NOT_WHOLE
  }
  const target = faultTarget(op)
  if (target !== null && !workload.some((event) => event.id === target)) {
    return `event ${target} isn't in this scenario`
  }
  return null
}

/** The op's numbers in a fixed order, so plans compare by meaning rather
 * than by how their objects happen to be laid out. */
function fieldsOf(op: FaultOp): number[] {
  if ('Duplicate' in op) return [op.Duplicate.event_id]
  if ('Reorder' in op) return [op.Reorder.event_id, op.Reorder.window]
  if ('Delay' in op) return [op.Delay.event_id, op.Delay.by]
  if ('Drop' in op) return [op.Drop.event_id]
  return [op.CrashRestart.at]
}

function sameFault(a: FaultOp, b: FaultOp): boolean {
  const left = fieldsOf(a)
  const right = fieldsOf(b)
  return (
    faultKind(a) === faultKind(b) &&
    left.length === right.length &&
    left.every((value, at) => value === right[at])
  )
}

/** Whether the editor's plan still matches, say, the story or seed plan. */
export function plansEqual(
  a: readonly FaultOp[],
  b: readonly FaultOp[],
): boolean {
  return a.length === b.length && a.every((op, at) => sameFault(op, b[at]))
}

/** The add form's fields. Numbers stay as typed until the fault is built. */
export interface FaultDraft {
  kind: FaultKind
  eventId: EventId
  window: string
  by: string
  at: string
}

export type DraftResult =
  { ok: true; op: FaultOp } | { ok: false; problem: string }

/** The fault the add form describes, or what's wrong with it. */
export function faultFromDraft(
  draft: FaultDraft,
  workload: readonly SimEvent[],
): DraftResult {
  const op = buildFault(draft)
  if (op === null) return { ok: false, problem: NOT_WHOLE }
  const problem = faultProblem(op, workload)
  return problem === null ? { ok: true, op } : { ok: false, problem }
}

function buildFault({
  kind,
  eventId,
  window,
  by,
  at,
}: FaultDraft): FaultOp | null {
  const event_id = eventId
  switch (kind) {
    case 'Duplicate':
      return { Duplicate: { event_id } }
    case 'Drop':
      return { Drop: { event_id } }
    case 'Reorder': {
      const deliveries = parseWholeNumber(window)
      return deliveries === null
        ? null
        : { Reorder: { event_id, window: deliveries } }
    }
    case 'Delay': {
      const ms = parseWholeNumber(by)
      return ms === null ? null : { Delay: { event_id, by: ms } }
    }
    case 'CrashRestart': {
      const ms = parseWholeNumber(at)
      return ms === null ? null : { CrashRestart: { at: ms } }
    }
  }
}

/** Where the plan the editor shows came from. `null` is the seed's plan,
 * which sim-api generates; an explicit plan that matches neither the story
 * nor the seed's known plan has been edited. */
export type PlanOrigin = 'story' | 'seed' | 'edited'

export function planOrigin(
  plan: readonly FaultOp[] | null,
  story: readonly FaultOp[],
  seedPlan: readonly FaultOp[] | null,
): PlanOrigin {
  if (plan === null) return 'seed'
  if (plansEqual(plan, story)) return 'story'
  if (seedPlan !== null && plansEqual(plan, seedPlan)) return 'seed'
  return 'edited'
}
