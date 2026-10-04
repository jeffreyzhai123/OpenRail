import { errorMessage } from '../lib/errors'

export function ErrorBanner({ error }: { error: unknown }) {
  if (error === null || error === undefined) {
    return null
  }
  return (
    <div className="error-banner" role="alert">
      {errorMessage(error)}
    </div>
  )
}
