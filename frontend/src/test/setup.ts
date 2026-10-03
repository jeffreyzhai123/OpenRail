import { cleanup } from '@testing-library/react'
import { afterEach } from 'vitest'

// Testing Library only auto-cleans when test globals are enabled; we keep
// them off and unmount explicitly so tests can't leak DOM into each other.
afterEach(cleanup)
