// Share links (README §6.2). The run's input, sim-api's `replay` string, and
// the trace hash it should reproduce live in the URL fragment
// (`#r=<replay>&h=<trace_hash>`), so static hosting needs no rewrites and the
// link never reaches a server log.

export interface ReplayLink {
  replay: string
  /** `null` when the link carries no hash to check against. */
  traceHash: string | null
}

export type Verification = 'verified' | 'mismatch' | 'unverified'

export function replayFragment(replay: string, traceHash: string): string {
  return `#${new URLSearchParams({ r: replay, h: traceHash })}`
}

/** The page's own address with this run's link as its fragment, replacing
 * any link it already holds. */
export function shareUrl(
  pageUrl: string,
  replay: string,
  traceHash: string,
): string {
  return pageUrl.split('#')[0] + replayFragment(replay, traceHash)
}

/** The link in a fragment (with or without its `#`), or `null` if it holds
 * no replay. */
export function parseReplayFragment(fragment: string): ReplayLink | null {
  const params = new URLSearchParams(fragment.replace(/^#/, ''))
  const replay = params.get('r')
  if (!replay) {
    return null
  }
  return { replay, traceHash: params.get('h') || null }
}

/** What the replay badge shows: whether the recomputed run reproduced the
 * hash the link promised. */
export function verify(expected: string | null, actual: string): Verification {
  if (expected === null) return 'unverified'
  return expected === actual ? 'verified' : 'mismatch'
}
