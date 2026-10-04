import { describe, expect, test } from 'vitest'
import type { JournalEntry, RunResponse } from '../api/types'
import { fixture, fixtures } from '../test/fixtures'
import { BalanceMismatchError, balancesByStep } from './balances'

type Fold = Pick<RunResponse, 'opening' | 'journal' | 'posted' | 'ledger'>

function transfer(
  source: number,
  kind: JournalEntry['kind'],
  from: string,
  to: string,
  cents: number,
): JournalEntry {
  return {
    source,
    intent: 'charge-1',
    kind,
    postings: [
      { account: from, delta: -cents },
      { account: to, delta: cents },
    ],
  }
}

/** The refund-before-capture case: the early refund (step 0) posts nothing,
 * the capture (step 1) posts, and the refund's redelivered copy (step 2)
 * posts. */
function redeliveredRefund(): Fold {
  return {
    opening: { 'external:card': 0, merchant: 0 },
    journal: [
      transfer(1, 'Capture', 'external:card', 'merchant', 500),
      transfer(2, 'Refund', 'merchant', 'external:card', 200),
    ],
    posted: [0, 1, 1],
    ledger: { accounts: { 'external:card': -300, merchant: 300 } },
  }
}

describe('every run fixture', () => {
  const runs = Object.keys(fixtures).filter(
    (name) => name.startsWith('run-') || name === 'replay.json',
  )

  test.each(runs)('%s starts at its opening and ends at its ledger', (name) => {
    const run = fixture(name) as RunResponse
    const steps = balancesByStep(run)
    expect(steps).toHaveLength(run.trace.length + 1)
    expect(steps[0]).toEqual(run.opening)
    expect(steps.at(-1)).toEqual(run.ledger.accounts)
  })
})

test('each posting lands at the delivery that made it', () => {
  const merchant = balancesByStep(redeliveredRefund()).map(
    (balances) => balances.merchant,
  )
  // Never negative: the refund lands after the capture, not at step 0.
  expect(merchant).toEqual([0, 0, 500, 300])
})

test('steps are separate snapshots', () => {
  const steps = balancesByStep(redeliveredRefund())
  steps[1].merchant = 999
  expect(steps[0].merchant).toBe(0)
  expect(steps[2].merchant).toBe(500)
})

describe('a fold that disagrees with its run throws', () => {
  test.each<[string, (run: Fold) => void]>([
    ['the final ledger differs', (run) => (run.ledger.accounts.merchant = 1)],
    ['posted counts too many entries', (run) => (run.posted[2] = 2)],
    ['the journal has unposted entries', (run) => (run.posted[2] = 0)],
    [
      'an entry touches an account the opening lacks',
      (run) => (run.journal[0].postings[1].account = 'nobody'),
    ],
  ])('%s', (_, breakIt) => {
    const run = redeliveredRefund()
    breakIt(run)
    expect(() => balancesByStep(run)).toThrow(BalanceMismatchError)
  })
})
