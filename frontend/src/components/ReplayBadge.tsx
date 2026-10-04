import type { Verification } from '../lib/replayLink'

interface ReplayBadgeProps {
  verification: Verification
  /** The trace hash the link promised. */
  expected: string | null
  /** The trace hash this recomputed run produced. */
  actual: string
}

/** Whether a share link's run, recomputed by sim-api, reproduced the trace
 * hash the link promised: the determinism claim, checked on every open. */
export function ReplayBadge({
  verification,
  expected,
  actual,
}: ReplayBadgeProps) {
  return (
    <section
      className={`panel replay-badge ${verification}`}
      aria-labelledby="replay-heading"
    >
      <h2 id="replay-heading">{HEADINGS[verification]}</h2>
      {verification === 'verified' && (
        <p>This run reproduces the trace hash the link promised.</p>
      )}
      {verification === 'mismatch' && (
        <>
          <p>This run doesn't reproduce the trace hash the link promised.</p>
          <dl>
            <dt>The link promised</dt>
            <dd>
              <code>{expected}</code>
            </dd>
            <dt>This run produced</dt>
            <dd>
              <code>{actual}</code>
            </dd>
          </dl>
        </>
      )}
      {verification === 'unverified' && (
        <p>The link carries no trace hash, so there's nothing to compare.</p>
      )}
    </section>
  )
}

const HEADINGS: Record<Verification, string> = {
  verified: '✓ Replay verified identical',
  mismatch: '✗ Determinism break',
  unverified: 'Replay unverified',
}
