import { render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, test, vi } from 'vitest'
import App from './App'
import { ApiError, NetworkError, type SimClient } from './api/client'
import {
  MAX_PLAN_FAULTS,
  type RunRequest,
  type RunResponse,
  type ScenarioSummary,
  type ShrinkRequest,
  type ShrinkResponse,
} from './api/types'
import { describeFault } from './lib/faultPlan'
import { parseReplayFragment, replayFragment } from './lib/replayLink'
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

/** The plan the editor shows, one description per fault. */
function shownFaults(): string[] {
  return screen
    .queryAllByRole('button', { name: /^Remove: / })
    .map((button) => button.getAttribute('aria-label')?.slice(8) ?? '')
}

async function editSeed(
  user: ReturnType<typeof userEvent.setup>,
  text: string,
) {
  const seed = screen.getByRole('textbox', { name: 'Seed' })
  await user.clear(seed)
  await user.type(seed, text)
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
      screen.getByRole('button', { name: 'Add fault' }),
      screen.getByRole('button', { name: /^Remove: / }),
    ]
    // `:disabled`, not the property: the lock comes from the fieldset.
    const locked = () => inputs.map((input) => input.matches(':disabled'))

    await user.click(screen.getByRole('button', { name: 'Run' }))
    expect(locked()).toEqual([true, true, true, true, true])

    answer(fixture('run-charge-retry-naive.json') as RunResponse)
    await screen.findByRole('heading', { name: /invariants/ })
    expect(locked()).toEqual([false, false, false, false, false])
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

  test('focus follows the selected step, and stops at the ends', async () => {
    const { user } = await renderApp()
    await run(user)
    const selected = () => document.activeElement?.getAttribute('aria-current')

    await user.click(screen.getByRole('button', { name: 'Opening balances' }))
    await user.keyboard('{ArrowRight}{ArrowRight}')
    expect(selected()).toBe('step')
    expect(document.activeElement?.textContent).toContain('2 s')

    await user.keyboard('{Home}{ArrowLeft}')
    expect(selected()).toBe('step')
    expect(document.activeElement?.textContent).toBe('Opening balances')
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
      screen.getByText(/Generated from seed 42 when you run/),
    ).toBeDefined()

    await run(user)
    expect(requests[0]).toMatchObject({ seed: 42, fault_plan: null })
  })

  test('after the run, a generated plan lists the faults sim-api used', async () => {
    // Any plan unlike the story's: the list must come from the response.
    const { original } = fixture('shrink.json') as ShrinkResponse
    const { user } = await renderApp({
      run: async () => ({
        ...(fixture('run-charge-retry-naive.json') as RunResponse),
        fault_plan: original,
      }),
    })
    const seed = screen.getByRole('textbox', { name: 'Seed' })
    await user.clear(seed)
    await user.type(seed, '42')
    await run(user)

    expect(screen.getByText('from seed 42')).toBeDefined()
    expect(shownFaults()).toEqual(original.map(describeFault))
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

describe('editing the fault plan', () => {
  const story = () =>
    (fixture('scenarios.json') as ScenarioSummary[])[0].story_plan

  test('an added Duplicate goes out in the next run request', async () => {
    const { user, requests } = await renderApp()
    await user.selectOptions(
      screen.getByRole('combobox', { name: 'Event' }),
      '3',
    )
    await user.click(screen.getByRole('button', { name: 'Add fault' }))

    expect(shownFaults()).toEqual([
      'Redeliver event 2 30 s later',
      'Redeliver event 3 30 s later',
    ])
    expect(screen.getByText('edited')).toBeDefined()
    await run(user)
    expect(requests[0].fault_plan).toEqual([
      ...story(),
      { Duplicate: { event_id: 3 } },
    ])
  })

  test('each kind asks for its own fields, and previews the fault', async () => {
    const { user, requests } = await renderApp()
    const type = screen.getByRole('combobox', { name: 'Type' })

    await user.selectOptions(type, 'CrashRestart')
    expect(screen.queryByRole('combobox', { name: 'Event' })).toBeNull()
    await user.clear(screen.getByRole('textbox', { name: 'At (ms)' }))
    await user.type(screen.getByRole('textbox', { name: 'At (ms)' }), '2000')
    expect(
      screen.getByText('Adds: Crash and restart the handler at 2 s'),
    ).toBeDefined()

    await user.selectOptions(type, 'Reorder')
    expect(
      screen.getByRole('textbox', { name: 'Window (deliveries)' }),
    ).toBeDefined()
    await user.click(screen.getByRole('button', { name: 'Add fault' }))
    await run(user)
    expect(requests[0].fault_plan).toEqual([
      ...story(),
      { Reorder: { event_id: 1, window: 2 } },
    ])
  })

  test("a number that isn't whole blocks Add, saying why", async () => {
    const { user } = await renderApp()
    await user.selectOptions(
      screen.getByRole('combobox', { name: 'Type' }),
      'Delay',
    )
    const by = screen.getByRole('textbox', { name: 'Delay by (ms)' })
    await user.clear(by)
    await user.type(by, '1.5')

    expect(by.getAttribute('aria-invalid')).toBe('true')
    expect(screen.getByRole('button', { name: 'Add fault' })).toHaveProperty(
      'disabled',
      true,
    )
    expect(
      screen.getByText(
        "Can't add: every number must be a whole number, 0 or more.",
      ),
    ).toBeDefined()
  })

  test('removing the story fault runs with no faults', async () => {
    const { user, requests } = await renderApp()
    await user.click(
      screen.getByRole('button', {
        name: 'Remove: Redeliver event 2 30 s later',
      }),
    )
    expect(screen.getByText('No faults.')).toBeDefined()
    await run(user)
    expect(requests[0].fault_plan).toEqual([])
  })

  test('an edit clears the shown run, which the old plan produced', async () => {
    const { user } = await renderApp()
    await run(user)
    await user.click(screen.getByRole('button', { name: /^Remove: / }))
    expect(screen.queryByRole('heading', { name: /invariants/ })).toBeNull()
  })

  test('reset to story plan undoes edits', async () => {
    const { user } = await renderApp()
    const reset = screen.getByRole('button', { name: 'Reset to story plan' })
    expect(reset).toHaveProperty('disabled', true)

    await user.click(screen.getByRole('button', { name: /^Remove: / }))
    await user.click(reset)
    expect(shownFaults()).toEqual(['Redeliver event 2 30 s later'])
    expect(screen.getByText('story plan')).toBeDefined()
  })

  test("a seed's plan is editable once a run has shown it", async () => {
    const { original } = fixture('shrink.json') as ShrinkResponse
    const { user, requests } = await renderApp({
      run: async (request) => {
        requests.push(request)
        return {
          ...(fixture('run-charge-retry-naive.json') as RunResponse),
          fault_plan: request.fault_plan ?? original,
        }
      },
    })
    await editSeed(user, '42')
    expect(screen.getByText(/Run once to edit it/)).toBeDefined()
    expect(screen.queryByRole('button', { name: 'Add fault' })).toBeNull()

    await run(user)
    await user.click(screen.getAllByRole('button', { name: /^Remove: / })[0])
    expect(screen.getByText('edited')).toBeDefined()
    await run(user)
    expect(requests[1].fault_plan).toEqual(original.slice(1))

    await user.click(screen.getByRole('button', { name: 'Reset to seed plan' }))
    expect(shownFaults()).toEqual(original.map(describeFault))
    expect(screen.getByText('from seed 42')).toBeDefined()
    await run(user)
    expect(requests[2].fault_plan).toBeNull()
  })

  test('a full plan takes no more faults', async () => {
    const full = Array.from({ length: MAX_PLAN_FAULTS }, () => ({
      Drop: { event_id: 1 },
    }))
    const { user } = await renderApp({
      run: async () => ({
        ...(fixture('run-charge-retry-naive.json') as RunResponse),
        fault_plan: full,
      }),
    })
    await editSeed(user, '42')
    await run(user)
    expect(
      screen.getByText(`A plan holds at most ${MAX_PLAN_FAULTS} faults.`),
    ).toBeDefined()
    expect(screen.getByRole('button', { name: 'Add fault' })).toHaveProperty(
      'disabled',
      true,
    )
  })
})

describe('share links', () => {
  const linked = () => fixture('replay.json') as RunResponse

  /** Opens the app on a share link, as a new tab would. */
  async function openLink(fragment: string, response: RunResponse = linked()) {
    window.history.replaceState(null, '', `/${fragment}`)
    const encoded: string[] = []
    const app = await renderApp({
      replay: async (replay) => {
        encoded.push(replay)
        return response
      },
    })
    return { ...app, encoded }
  }

  afterEach(() => {
    window.history.replaceState(null, '', '/')
    vi.restoreAllMocks()
  })

  test('Share copies a link that replays this run', async () => {
    const { user } = await renderApp()
    expect(screen.queryByRole('button', { name: 'Share' })).toBeNull()
    await run(user)
    await user.click(screen.getByRole('button', { name: 'Share' }))

    expect(screen.getByText('Link copied.')).toBeDefined()
    const copied = new URL(await navigator.clipboard.readText())
    const response = fixture('run-charge-retry-naive.json') as RunResponse
    expect(parseReplayFragment(copied.hash)).toEqual({
      replay: response.replay,
      traceHash: response.trace_hash,
    })
  })

  test('when copying fails, the link is shown to copy by hand', async () => {
    const { user } = await renderApp()
    await run(user)
    vi.spyOn(navigator.clipboard, 'writeText').mockRejectedValue(
      new Error('denied'),
    )
    await user.click(screen.getByRole('button', { name: 'Share' }))

    expect(await screen.findByText(/Couldn't copy/)).toBeDefined()
    const link = screen.getByRole('textbox', { name: 'Share link' })
    expect((link as HTMLInputElement).value).toContain(linked().replay)
  })

  test('opening a link replays it, verified, with its inputs loaded', async () => {
    const response = linked()
    const { encoded } = await openLink(
      replayFragment(response.replay, response.trace_hash),
    )
    expect(
      await screen.findByRole('heading', {
        name: '✓ Replay verified identical',
      }),
    ).toBeDefined()
    expect(encoded).toEqual([response.replay])
    expect(
      (screen.getByRole('combobox', { name: 'Scenario' }) as HTMLSelectElement)
        .value,
    ).toBe(response.scenario_id)
    expect(
      (screen.getByRole('textbox', { name: 'Seed' }) as HTMLInputElement).value,
    ).toBe(String(response.seed))
    expect(screen.getByRole('radio', { name: 'Naive' })).toHaveProperty(
      'checked',
      true,
    )
    expect(shownFaults()).toEqual(response.fault_plan.map(describeFault))
  })

  test("a plan that isn't the story's is labelled as the link's", async () => {
    const response = { ...linked(), fault_plan: [{ Drop: { event_id: 3 } }] }
    await openLink(
      replayFragment(response.replay, response.trace_hash),
      response,
    )
    await screen.findByRole('heading', { name: /Replay/ })
    expect(screen.getByText('from the link')).toBeDefined()
  })

  test('a different hash is a determinism break, showing both', async () => {
    const response = linked()
    const promised = '0'.repeat(64)
    await openLink(replayFragment(response.replay, promised))
    expect(
      await screen.findByRole('heading', { name: '✗ Determinism break' }),
    ).toBeDefined()
    expect(screen.getByText(promised)).toBeDefined()
    expect(screen.getByText(response.trace_hash)).toBeDefined()
  })

  test('a link without a hash replays unverified', async () => {
    await openLink(`#r=${linked().replay}`)
    expect(
      await screen.findByRole('heading', { name: 'Replay unverified' }),
    ).toBeDefined()
  })

  test("a link this server can't read says so, and the app still runs", async () => {
    window.history.replaceState(null, '', '/#r=2.abc&h=ff')
    await renderApp({
      replay: () =>
        Promise.reject(
          new ApiError(422, 'unsupported_encoding_version', 'server wording'),
        ),
    })
    const banner = await screen.findByRole('alert')
    expect(banner.textContent).toBe(
      "This share link uses an encoding version this server doesn't read.",
    )
    expect(screen.getByRole('button', { name: 'Run' })).toHaveProperty(
      'disabled',
      false,
    )
  })

  test('a link pasted into the open tab replays too', async () => {
    const response = linked()
    const { encoded } = await openLink('')
    window.location.hash = replayFragment(response.replay, response.trace_hash)
    expect(
      await screen.findByRole('heading', {
        name: '✓ Replay verified identical',
      }),
    ).toBeDefined()
    expect(encoded).toEqual([response.replay])
  })

  test("a link that fails leaves no earlier link's run on screen", async () => {
    const response = linked()
    window.history.replaceState(
      null,
      '',
      `/${replayFragment(response.replay, response.trace_hash)}`,
    )
    await renderApp({
      replay: async (replay) => {
        if (replay.startsWith('2.')) {
          throw new ApiError(422, 'unsupported_encoding_version', 'v2')
        }
        return response
      },
    })
    await screen.findByRole('heading', { name: /Replay verified/ })

    window.location.hash = '#r=2.abc&h=ff'
    expect(await screen.findByRole('alert')).toBeDefined()
    expect(screen.queryByRole('heading', { name: /Replay/ })).toBeNull()
    expect(screen.queryByRole('heading', { name: /invariants/ })).toBeNull()
  })

  test('scenarios arriving after unmount leave no link listener', async () => {
    let arrive: (scenarios: ScenarioSummary[]) => void = () => {}
    const encoded: string[] = []
    const { client } = stubClient({
      // Ignores the abort signal, like a response already on its way.
      scenarios: () => new Promise((resolve) => (arrive = resolve)),
      replay: async (replay) => {
        encoded.push(replay)
        return linked()
      },
    })
    const { unmount } = render(<App client={client} />)
    unmount()
    arrive(fixture('scenarios.json') as ScenarioSummary[])
    await Promise.resolve()

    window.location.hash = replayFragment(linked().replay, linked().trace_hash)
    await new Promise((resolve) => setTimeout(resolve, 0))
    expect(encoded).toEqual([])
  })

  test('running after a replay drops the badge', async () => {
    const response = linked()
    const { user } = await openLink(
      replayFragment(response.replay, response.trace_hash),
    )
    await screen.findByRole('heading', { name: /Replay/ })
    await run(user)
    expect(screen.queryByRole('heading', { name: /Replay/ })).toBeNull()
  })
})

describe('shrinking', () => {
  const shrinkFixture = () => fixture('shrink.json') as ShrinkResponse

  /** Runs the story under naive, with a stub shrinker answering `answer`. */
  async function failingRun(answer = shrinkFixture()) {
    const shrinks: ShrinkRequest[] = []
    const app = await renderApp({
      shrink: async (request) => {
        shrinks.push(request)
        return answer
      },
    })
    await run(app.user)
    return { ...app, shrinks }
  }

  test('only failed invariants offer a shrink', async () => {
    const { user } = await failingRun()
    const buttons = screen.getAllByRole('button', { name: /^Shrink the plan/ })
    expect(buttons.map((button) => button.getAttribute('aria-label'))).toEqual([
      'Shrink the plan on single_capture_per_intent',
      'Shrink the plan on single_entry_per_source_event',
    ])
    await user.click(screen.getByRole('radio', { name: 'Hardened' }))
    await run(user)
    expect(
      screen.queryByRole('button', { name: /^Shrink the plan/ }),
    ).toBeNull()
  })

  test("a shrink sends the run's inputs and the invariant", async () => {
    const { user, shrinks, requests } = await failingRun()
    await user.click(
      screen.getByRole('button', {
        name: 'Shrink the plan on single_entry_per_source_event',
      }),
    )
    await screen.findByRole('heading', { name: /Reduced plan/ })
    expect(shrinks).toEqual([
      { ...requests[0], invariant: 'single_entry_per_source_event' },
    ])
  })

  test('the reduced plan shows what was kept, never claiming "minimal"', async () => {
    const { user } = await failingRun()
    await user.click(
      screen.getAllByRole('button', { name: /^Shrink the plan/ })[1],
    )
    const view = (
      await screen.findByRole('heading', { name: /Reduced plan/ })
    ).closest('section') as HTMLElement

    expect(within(view).getByText(/1 of 3 faults still break/)).toBeDefined()
    expect(
      within(view)
        .getAllByRole('listitem')
        .map((item) => item.className),
    ).toEqual(['removed', 'removed', 'kept'])
    expect(within(view).getByText(/3 candidates tried/)).toBeDefined()
    expect(document.body.textContent).not.toMatch(/minimal/i)
  })

  test('Load reduced run shows its run and makes its plan current', async () => {
    const response = shrinkFixture()
    const { user, requests } = await failingRun()
    await user.click(
      screen.getAllByRole('button', { name: /^Shrink the plan/ })[1],
    )
    await user.click(
      await screen.findByRole('button', { name: 'Load reduced run' }),
    )

    expect(screen.queryByRole('heading', { name: /Reduced plan/ })).toBeNull()
    expect(
      screen.getByRole('heading', { name: /invariants/ }).textContent,
    ).toBe('1 of 5 invariants failed')
    expect(shownFaults()).toEqual(response.shrunk.map(describeFault))
    expect(screen.getByText('reduced')).toBeDefined()
    await run(user)
    expect(requests.at(-1)?.fault_plan).toEqual(response.shrunk)
  })

  test('a plan that needs every fault says so, with nothing to load', async () => {
    const story = (fixture('scenarios.json') as ScenarioSummary[])[0].story_plan
    const { user } = await failingRun({
      ...shrinkFixture(),
      original: story,
      shrunk: story,
      candidates_tried: 1,
    })
    await user.click(
      screen.getAllByRole('button', { name: /^Shrink the plan/ })[0],
    )
    expect(await screen.findByText(/Every fault is needed/)).toBeDefined()
    expect(screen.getByText(/1 candidate tried/)).toBeDefined()
    expect(
      screen.queryByRole('button', { name: 'Load reduced run' }),
    ).toBeNull()
    await user.click(screen.getByRole('button', { name: 'Close' }))
    expect(screen.queryByRole('heading', { name: /Reduced plan/ })).toBeNull()
  })

  test('a shrink in flight locks the inputs and says which', async () => {
    let answer: (response: ShrinkResponse) => void = () => {}
    const { user } = await renderApp({
      shrink: () => new Promise((resolve) => (answer = resolve)),
    })
    await run(user)
    await user.click(
      screen.getAllByRole('button', { name: /^Shrink the plan/ })[0],
    )

    expect(screen.getByText('Shrinking…')).toBeDefined()
    expect(screen.getByRole('button', { name: 'Run' })).toHaveProperty(
      'disabled',
      true,
    )
    expect(
      screen.getByRole('combobox', { name: 'Scenario' }).matches(':disabled'),
    ).toBe(true)
    answer(shrinkFixture())
    await screen.findByRole('heading', { name: /Reduced plan/ })
    expect(screen.getByRole('button', { name: 'Run' })).toHaveProperty(
      'disabled',
      false,
    )
    expect(screen.queryByText('Shrinking…')).toBeNull()
  })

  test('a shrink sim-api refuses gets its banner and leaves the run', async () => {
    const { user } = await renderApp({
      shrink: () =>
        Promise.reject(new ApiError(422, 'does_not_fail', 'server wording')),
    })
    await run(user)
    await user.click(
      screen.getAllByRole('button', { name: /^Shrink the plan/ })[0],
    )

    expect((await screen.findByRole('alert')).textContent).toBe(
      "That plan doesn't fail the invariant, so there's nothing to shrink.",
    )
    expect(screen.getByRole('heading', { name: /invariants/ })).toBeDefined()
    expect(
      screen.getAllByRole('button', { name: /^Shrink the plan/ })[0],
    ).toHaveProperty('disabled', false)
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
