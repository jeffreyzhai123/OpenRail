import { useState } from 'react'

type CopyState = 'idle' | 'copied' | 'failed'

/** Copies the shown run's share link. If the clipboard isn't available
 * (an insecure origin, or permission denied), it shows the link to copy by
 * hand instead. */
export function ShareButton({ url }: { url: string }) {
  const [copy, setCopy] = useState<CopyState>('idle')

  function share() {
    // Absent outside a secure context, so reached through optional chaining.
    const write = navigator.clipboard?.writeText(url)
    if (!write) {
      setCopy('failed')
      return
    }
    write.then(
      () => setCopy('copied'),
      () => setCopy('failed'),
    )
  }

  return (
    <div className="share">
      <button type="button" onClick={share}>
        Share
      </button>
      <p className="hint" aria-live="polite">
        {copy === 'copied' && 'Link copied.'}
        {copy === 'failed' && "Couldn't copy. Copy the link below."}
      </p>
      {copy !== 'idle' && (
        <input
          className="share-link"
          aria-label="Share link"
          readOnly
          value={url}
          onFocus={(event) => event.target.select()}
        />
      )}
    </div>
  )
}
