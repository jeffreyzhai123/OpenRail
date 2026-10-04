import { render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, test } from 'vitest'
import App from './App'
import { ApiError, NetworkError, type SimClient } from './api/client'
import type { RunRequest, RunResponse, ScenarioSummary } from './api/types'
import { fixture } from './test/fixtures'

const notYet = () => Promise.reject(new Error('not used by the core loop'))

/** A client serving the golden fixtures. Each run answers with the story
 * run for its scenario and handler; tests that send other plans only check
 * the request. */
function stubClient(overrides: Partial<SimClient> = {}) {
  const requests: RunRequest[] = []
  const client: SimClient = {
    scenarios: async () => fixture('scenarios.json') as ScenarioSummary[],
    run: async (request) => {
      requests.push(request)
      const name = `run-${request.scenario_id}-${request.handler}.json`
      return fixture(name) as RunResponse
    },
    replay: notYet,
    shrink: notYet,
    sweep: notYet,
    ...overrides,
  }
  return { client, requests }
}

async function renderApp(overrides: Partial<SimClient> = {}) {
  const user = userEvent.setup()
  const stub = stubClient(overrides)
  render(<App client={stub.client} />)
  await screen.findByRole('option', { name: 'Charge retried after timeout' })
  return { user, ...stub }
}

async function run(user: ReturnType<typeof userEvent.setup>) {
  await user.click(screen.getByRole('button', { name: 'Run' }))
  return screen.findByRole('heading', { name: /invariants/ })
}

function merchantBalance(): string {
  const row = screen.getByRole('row', { name: /merchant/ })
  return within(row).getAllByRole('cell')[0].textContent ?? ''
}

test('renders the playground heading', async () => {
  await renderApp()
  expect(
    screen.getByRole('heading', { name: 'Rails Sim Playground' }),
  ).toBeDefined()
})

test('opens on the first scenario and its story plan', async () => {
  await renderApp()
  expect(screen.getByText(/redelivers it 30 seconds later/)).toBeDefined()
  expect(screen.getByText('Redeliver event 2 30 s later')).toBeDefined()
})

describe('running', () => {
  test('naive breaks on the story, hardened holds', async () => {
    const { user } = await renderApp()

    const naive = await run(user)
    expect(naive.textContent).toBe('2 of 5 invariants failed')
    const failure = screen.getByText('single_capture_per_intent').closest('li')
    expect(failure?.textContent).toContain('failed')

    await user.click(screen.getByRole('radio', { name: 'Hardened' }))
    const hardened = await run(user)
    expect(hardened.textContent).toBe('All 5 invariants hold')
  })

  test("the request carries the scenario's story plan", async () => {
    const { user, requests } = await renderApp()
    await run(user)
    const scenario = (fixture('scenarios.json') as ScenarioSummary[])[0]
    expect(requests).toEqual([
      {
        scenario_id: scenario.id,
        seed: 0,
        handler: 'naive',
        fault_plan: scenario.story_plan,
      },
    ])
  })

  test('the timeline marks a redelivery that posted nothing', async () => {
    const { user } = await renderApp()
    await user.click(screen.getByRole('radio', { name: 'Hardened' }))
    await run(user)
    const deliveries = screen.getByRole('list', { name: 'Deliveries' })
    const copy = within(deliveries)
      .getAllByRole('button')
      .find((row) => row.textContent?.includes('redelivery'))
    expect(copy?.textContent).toContain('posted nothing')
  })

  test('inputs lock while a run is in flight', async () => {
    let answer: (response: RunResponse) => void = () => {}
    const { user } = await renderApp({
      run: () => new Promise((resolve) => (answer = resolve)),
    })
    const inputs = [
      screen.getByRole('combobox', { name: 'Scenario' }),
      screen.getByRole('textbox', { name: 'Seed' }),
      screen.getByRole('radio', { name: 'Hardened' }),
    ]
    // `:disabled`, not the property: the lock comes from the fieldset.
    const locked = () => inputs.map((input) => input.matches(':disabled'))

    await user.click(screen.getByRole('button', { name: 'Run' }))
    expect(locked()).toEqual([true, true, true])

    answer(fixture('run-charge-retry-naive.json') as RunResponse)
    await screen.findByRole('heading', { name: /invariants/ })
    expect(locked()).toEqual([false, false, false])
  })
})

describe('scrubbing', () => {
  test('arrow keys and clicks move the balances through the run', async () => {
    const { user } = await renderApp()
    await run(user)
    // Captured, redelivered and captured again, then $10 refunded.
    expect(merchantBalance()).toBe('$90.00')

    const deliveries = screen.getByRole('list', { name: 'Deliveries' })
    await user.click(
      within(deliveries).getAllByRole('button').at(-1) as HTMLElement,
    )
    await user.keyboard('{ArrowLeft}')
    expect(merchantBalance()).toBe('$100.00')

    await user.click(screen.getByRole('button', { name: 'Opening balances' }))
    expect(merchantBalance()).toBe('$0.00')
    expect(screen.getByText('Before any delivery.')).toBeDefined()
  })
})

describe('inputs', () => {
  test('picking a scenario shows its description and story', async () => {
    const { user } = await renderApp()
    await user.selectOptions(
      screen.getByRole('combobox', { name: 'Scenario' }),
      'refund-before-capture',
    )
    expect(screen.getByText(/the refund arrives first/)).toBeDefined()
    expect(
      screen.getByText('Reverse the 2 deliveries starting at event 2'),
    ).toBeDefined()
  })

  test('changing the seed resets the plan to the generated one', async () => {
    const { user, requests } = await renderApp()
    const seed = screen.getByRole('textbox', { name: 'Seed' })
    await user.clear(seed)
    await user.type(seed, '42')
    expect(
      screen.getByText('Generated from seed 42 when you run.'),
    ).toBeDefined()

    await run(user)
    expect(requests[0]).toMatchObject({ seed: 42, fault_plan: null })
  })

  test("a seed that isn't a u32 can't run", async () => {
    const { user } = await renderApp()
    const seed = screen.getByRole('textbox', { name: 'Seed' })
    await user.clear(seed)
    await user.type(seed, '-1')
    expect(screen.getByRole('button', { name: 'Run' })).toHaveProperty(
      'disabled',
      true,
    )
    expect(screen.getByText(/A seed is a whole number/)).toBeDefined()
  })
})

describe('errors', () => {
  test("sim-api's timeout gets the banner's own words", async () => {
    const { user } = await renderApp({
      run: () => Promise.reject(new ApiError(503, 'timeout', 'server wording')),
    })
    await user.click(screen.getByRole('button', { name: 'Run' }))
    const banner = await screen.findByRole('alert')
    expect(banner.textContent).toBe(
      'The simulation took too long. Try again in a moment.',
    )
  })

  test('an unreachable sim-api says how to start it', async () => {
    render(
      <App
        client={
          stubClient({
            scenarios: () =>
              Promise.reject(new NetworkError(new TypeError('x'))),
          }).client
        }
      />,
    )
    const banner = await screen.findByRole('alert')
    expect(banner.textContent).toContain('cargo run -p sim-api')
  })
})
