# Rails Sim Playground: frontend

React + TypeScript UI for the simulator. It talks to the stateless `sim-api` over HTTP. The design, the API contract and what's done so far are in [`specs/v1-frontend-tasks.md`](../specs/v1-frontend-tasks.md), and the overall design is in the [root README](../README.md).

## Developing

Every number the app shows comes from a real run, so start sim-api first. There's no offline or fixtures mode.

```
cargo run -p sim-api   # from the repo root; listens on :3000
npm install
npm run dev            # Vite dev server on :5173; proxies /api to :3000
```

The app calls `VITE_API_BASE_URL`, which defaults to `/api`, the dev proxy. A production build must set it to the deployed sim-api's URL. If sim-api isn't running, the app says so and shows how to start it.

## Checks

```
npm run check    # typecheck, lint, format check, tests: run on every change
npm run build    # production build into dist/
```

The tests never touch the network. They render the app against a stub client that serves the golden fixtures in `src/api/fixtures/`, which sim-api's own tests generate (`UPDATE_FIXTURES=1 cargo test -p sim-api --test fixtures`).
