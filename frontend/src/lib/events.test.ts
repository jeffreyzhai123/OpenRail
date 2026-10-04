import { expect, test } from 'vitest'
import type { EventKind, JournalEntry } from '../api/types'
import { describeEntry, describeEvent } from './events'

test.each<[EventKind, string]>([
  [
    { Card: { Authorized: { charge_id: 1, amount: 5_000 } } },
    'Card authorized · charge 1 · $50.00',
  ],
  [
    { Card: { Captured: { charge_id: 1, amount: 5_000 } } },
    'Card captured · charge 1 · $50.00',
  ],
  [
    { Card: { Refunded: { charge_id: 2, amount: 1_000 } } },
    'Card refunded · charge 2 · $10.00',
  ],
  [
    { Ach: { Initiated: { entry_id: 1, amount: 20_000 } } },
    'ACH initiated · entry 1 · $200.00',
  ],
  [{ Ach: { Batched: { entry_id: 1 } } }, 'ACH batched · entry 1'],
  [{ Ach: { Settled: { entry_id: 1 } } }, 'ACH settled · entry 1'],
  [
    { Ach: { Returned: { entry_id: 1, code: 'R01', amount: 20_000 } } },
    'ACH returned (R01) · entry 1 · $200.00',
  ],
  [
    { Ach: { Returned: { entry_id: 1, code: { Other: 'R10' }, amount: 5 } } },
    'ACH returned (R10) · entry 1 · $0.05',
  ],
])('%j is "%s"', (kind, expected) => {
  expect(describeEvent(kind)).toBe(expected)
})

test('an entry lists its signed legs', () => {
  const entry: JournalEntry = {
    source: 2,
    intent: 'charge-1',
    kind: 'Capture',
    postings: [
      { account: 'external:card', delta: -5_000 },
      { account: 'merchant', delta: 5_000 },
    ],
  }
  expect(describeEntry(entry)).toBe(
    'Capture charge-1: external:card -$50.00, merchant +$50.00',
  )
})
