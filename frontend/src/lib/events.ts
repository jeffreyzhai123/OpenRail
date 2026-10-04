// One-line descriptions of delivered events and their postings, for the
// timeline.

import type { AchReturnCode, EventKind, JournalEntry } from '../api/types'
import { formatDollars } from './money'

/** E.g. "Card captured · charge 1 · $50.00". */
export function describeEvent(kind: EventKind): string {
  if ('Card' in kind) {
    const card = kind.Card
    if ('Authorized' in card) {
      const { charge_id, amount } = card.Authorized
      return `Card authorized · charge ${charge_id} · ${formatDollars(amount)}`
    }
    if ('Captured' in card) {
      const { charge_id, amount } = card.Captured
      return `Card captured · charge ${charge_id} · ${formatDollars(amount)}`
    }
    const { charge_id, amount } = card.Refunded
    return `Card refunded · charge ${charge_id} · ${formatDollars(amount)}`
  }
  const ach = kind.Ach
  if ('Initiated' in ach) {
    const { entry_id, amount } = ach.Initiated
    return `ACH initiated · entry ${entry_id} · ${formatDollars(amount)}`
  }
  if ('Batched' in ach) return `ACH batched · entry ${ach.Batched.entry_id}`
  if ('Settled' in ach) return `ACH settled · entry ${ach.Settled.entry_id}`
  const { entry_id, code, amount } = ach.Returned
  return `ACH returned (${returnCode(code)}) · entry ${entry_id} · ${formatDollars(amount)}`
}

function returnCode(code: AchReturnCode): string {
  return typeof code === 'string' ? code : code.Other
}

/** E.g. "Capture charge-1: external:card -$50.00, merchant +$50.00". */
export function describeEntry(entry: JournalEntry): string {
  const legs = entry.postings.map(
    ({ account, delta }) =>
      `${account} ${delta > 0 ? '+' : ''}${formatDollars(delta)}`,
  )
  return `${entry.kind} ${entry.intent}: ${legs.join(', ')}`
}
