import { useEffect, useReducer, useState, type Dispatch } from 'react'
import { Latest, isAbortError, type SimClient } from './api/client'
import type { RunResponse } from './api/types'
import { BalancePanel } from './components/BalancePanel'
import { Controls } from './components/Controls'
import { ErrorBanner } from './components/ErrorBanner'
import { InvariantPanel } from './components/InvariantPanel'
import { ReplayBadge } from './components/ReplayBadge'
import { Timeline } from './components/Timeline'
import { balancesByStep, entriesByStep, type Balances } from './lib/balances'
import { parseReplayFragment, shareUrl } from './lib/replayLink'
import { randomSeed } from './lib/seed'
import {
  initialState,
  reducer,
  runRequest,
  type Action,
} from './state/appState'

/** Dispatches how a run-like call ends: `done` with its balances, or the
 * failure for the banner. */
function settle(
  call: Promise<RunResponse>,
  dispatch: Dispatch<Action>,
  done: (response: RunResponse, balances: Balances[]) => Action,
) {
  call.then(
    (response) => {
      try {
        dispatch(done(response, balancesByStep(response)))
      } catch (error) {
        dispatch({ type: 'failed', error })
      }
    },
    (error: unknown) => {
      // A superseded call was aborted on purpose; its replacement reports.
      if (!isAbortError(error)) dispatch({ type: 'failed', error })
    },
  )
}

/** The playground. Every number it shows comes from `client`: sim-api in the
 * app, a stub serving the golden fixtures in tests. */
function App({ client }: { client: SimClient }) {
  const [state, dispatch] = useReducer(reducer, initialState)
  const [runs] = useState(() => new Latest())

  useEffect(() => {
    const controller = new AbortController()
    // A share link in the fragment replays once the scenarios are in, and
    // again whenever a new link is pasted into this tab.
    const replayLink = () => {
      const link = parseReplayFragment(window.location.hash)
      if (link === null) return
      dispatch({ type: 'replayStarted' })
      settle(
        runs.start((signal) => client.replay(link.replay, { signal })),
        dispatch,
        (response, balances) => ({
          type: 'replaySucceeded',
          response,
          balances,
          expected: link.traceHash,
        }),
      )
    }
    client.scenarios({ signal: controller.signal }).then(
      (scenarios) => {
        // Cleaned up already, so the listener would never be removed.
        if (controller.signal.aborted) return
        dispatch({ type: 'scenariosLoaded', scenarios })
        replayLink()
        window.addEventListener('hashchange', replayLink)
      },
      (error: unknown) => {
        if (!isAbortError(error)) dispatch({ type: 'failed', error })
      },
    )
    return () => {
      controller.abort()
      window.removeEventListener('hashchange', replayLink)
    }
  }, [client, runs])

  const request = runRequest(state)

  function startRun() {
    if (request === null) return
    dispatch({ type: 'runStarted' })
    settle(
      runs.start((signal) => client.run(request, { signal })),
      dispatch,
      (response, balances) => ({ type: 'runSucceeded', response, balances }),
    )
  }

  const { run } = state
  const share = run
    ? shareUrl(
        window.location.href,
        run.response.replay,
        run.response.trace_hash,
      )
    : null
  return (
    <>
      <header className="app-header">
        <h1>Rails Sim Playground</h1>
        <p>
          Deterministic payments simulator. No real money moves, and no
          compliance claims are made.
        </p>
      </header>
      <ErrorBanner error={state.error} />
      <main className="layout">
        <Controls
          scenarios={state.scenarios}
          scenarioId={state.scenarioId}
          seedText={state.seedText}
          seed={state.seed}
          handler={state.handler}
          plan={state.plan}
          seedPlan={state.seedPlan}
          linkPlan={state.linkPlan}
          running={state.status === 'running'}
          canRun={request !== null && state.status === 'ready'}
          onScenario={(id) => dispatch({ type: 'scenarioPicked', id })}
          onSeed={(text) => dispatch({ type: 'seedEdited', text })}
          onRandomSeed={() =>
            dispatch({ type: 'seedEdited', text: String(randomSeed()) })
          }
          onHandler={(handler) => dispatch({ type: 'handlerPicked', handler })}
          onPlanChange={(plan) => dispatch({ type: 'planEdited', plan })}
          onPlanReset={(to) => dispatch({ type: 'planReset', to })}
          onRun={startRun}
          shareUrl={share}
        />
        <div className="results">
          {run ? (
            <>
              {run.link && (
                <ReplayBadge
                  verification={run.link.verification}
                  expected={run.link.expected}
                  actual={run.response.trace_hash}
                />
              )}
              <InvariantPanel invariants={run.response.invariants} />
              <div className="trace">
                <Timeline
                  trace={run.response.trace}
                  entries={entriesByStep(run.response)}
                  step={run.step}
                  onStep={(step) => dispatch({ type: 'stepSelected', step })}
                />
                <BalancePanel balances={run.balances} step={run.step} />
              </div>
            </>
          ) : (
            <p className="placeholder">{PLACEHOLDERS[state.status]}</p>
          )}
        </div>
      </main>
    </>
  )
}

const PLACEHOLDERS = {
  loading: 'Loading scenarios…',
  running: 'Running…',
  ready: 'Pick a scenario and run it to see what happens.',
}

export default App
