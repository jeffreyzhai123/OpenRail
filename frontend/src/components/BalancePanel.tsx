import type { Balances } from '../lib/balances'
import { formatDollars } from '../lib/money'

interface BalancePanelProps {
  /** From balancesByStep: index 0 is the opening. */
  balances: Balances[]
  step: number
}

/** Balances after the selected delivery, with what that delivery changed. */
export function BalancePanel({ balances, step }: BalancePanelProps) {
  const now = balances[step]
  const before = step > 0 ? balances[step - 1] : null
  return (
    <section className="panel balances" aria-labelledby="balances-heading">
      <h2 id="balances-heading">
        {step === 0 ? 'Opening balances' : `Balances after delivery ${step}`}
      </h2>
      <table>
        <thead>
          <tr>
            <th scope="col">Account</th>
            <th scope="col">Balance</th>
            <th scope="col">Change</th>
          </tr>
        </thead>
        <tbody>
          {Object.keys(now).map((account) => {
            const change = before ? now[account] - before[account] : 0
            return (
              <tr
                key={account}
                className={change === 0 ? undefined : 'changed'}
              >
                <th scope="row">
                  <code>{account}</code>
                </th>
                <td>{formatDollars(now[account])}</td>
                <td>
                  {change === 0
                    ? ''
                    : `${change > 0 ? '+' : ''}${formatDollars(change)}`}
                </td>
              </tr>
            )
          })}
        </tbody>
      </table>
    </section>
  )
}
