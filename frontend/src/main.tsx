import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import './styles.css'
import App from './App.tsx'
import { HttpClient } from './api/client.ts'

const root = document.getElementById('root')
if (!root) {
  throw new Error('index.html is missing the #root element')
}

// Defaults to /api, which the Vite dev proxy forwards to a local sim-api.
const client = new HttpClient({ baseUrl: import.meta.env.VITE_API_BASE_URL })

createRoot(root).render(
  <StrictMode>
    <App client={client} />
  </StrictMode>,
)
