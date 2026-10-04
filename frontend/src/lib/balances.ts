// Balances after each step of a run, for the timeline's balance panel.
// Display only: sim-api's ledger is the truth, and a fold that disagrees with
// it throws rather than showing numbers the server never had.

import type { Cents, RunResponse } from '../api/types'

export type Balances = Record<string, Cents>

/** The fold doesn't match the run it came from: contract drift. */
export class BalanceMismatchError extends Error {
  constructor(message: string) {
    super(message)
    this.name = 'BalanceMismatchError'
  }
}

/** Index 0 is the opening, index k the balances after the first k
 * deliveries, and the last the final ledger. Each step folds in exactly the
 * journal entries it posted (`posted`), so a posting is never shown at the
 * wrong delivery, even when an event is delivered twice. */
export function balancesByStep(
  run: Pick<RunResponse, 'opening' | 'journal' | 'posted' | 'ledger'>,
): Balances[] {
  let current: Balances = { ...run.opening }
  const steps = [current]
  let next = 0

  for (const count of run.posted) {
    const entries = run.journal.slice(next, next + count)
    if (entries.length !== count) {
      throw new BalanceMismatchError(
        'posted counts more entries than the journal has',
      )
    }
    next += count
    current = { ...current }
    for (const { postings } of entries) {
      for (const { account, delta } of postings) {
        if (!(account in current)) {
          throw new BalanceMismatchError(`${account} isn't in the opening`)
        }
        current[account] = current[account] + delta
      }
    }
    steps.push(current)
  }

  if (next !== run.journal.length) {
    throw new BalanceMismatchError('the journal has entries no step posted')
  }
  if (!sameBalances(current, run.ledger.accounts)) {
    throw new BalanceMismatchError(
      "the journal doesn't add up to the final ledger",
    )
  }
  return steps
}

function sameBalances(a: Balances, b: Balances): boolean {
  const accounts = Object.keys(a)
  return (
    accounts.length === Object.keys(b).length &&
    accounts.every((account) => account in b && a[account] === b[account])
  )
}
