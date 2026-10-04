# Plan: Deploy (S5)

## Context
README §3 V1 ends with "deployed and smoke-tested against the real backend", and D1 picked Fly.io for the API. Nothing deploy-related exists yet: no `Dockerfile`, no `fly.toml`, no `.github/`. The roadmap's housekeeping also still lists "no CI".

What's ready:
- `sim-api` binds `0.0.0.0:$PORT`, reads `ALLOWED_ORIGIN` for CORS, answers `GET /health`, and shuts down gracefully on SIGTERM (`sim-api-plan.md`).
- Golden fixtures of its responses are committed in `frontend/src/api/fixtures/`.
- The frontend's core loop, fault plan editor and share links run against a local sim-api (steps 1–5). Steps 6–7 are still open, and step 8 is this plan's smoke test.

Constraints found while planning:
- **The repo is private**, and your access is write, not admin. So **GitHub Pages is out**: it needs a paid plan and an admin to enable it.
- **No `fly` or `docker` is installed locally.** Fly's remote builder builds the images, and GitHub's runners have Docker for checking that they build.

**Status (2026-10-03):** **paused** at the user's request, to focus on frontend steps 1–7. Decisions F, R and K and deviations 1–3 are approved. DP1 (CI) was written on branch `deploy`, then reverted. Nothing deploy-related is in the repo, and the local `deploy` branch holds only the approval commit. When this resumes, start again at DP1.

## The principle behind most decisions below
**Ship the same bytes everywhere, and prove it on every deploy.** The API's whole promise is that a replay link verifies anywhere. So the deployed release build on Linux must answer exactly like the fixtures generated locally, and the smoke test checks that byte for byte against those fixtures.

Everything is code in the repo: images, Fly configs, CI and the smoke script. That leaves only the steps that handle credentials to be run by hand, and those are **yours to run**: I don't create accounts or handle tokens.

## Decisions (approved by the user, 2026-10-03)
### F: where the frontend is hosted
**A. A second Fly app serving the static build with nginx (`openrail-web`)**
- ✅ One vendor and one CLI, and the API is unchanged. It uses the CORS path sim-api already has, as README §2 intends ("API + static hosting").
- ✅ Works with a private repo.
- ❌ Two apps to name and deploy. No CDN, which is fine for a demo.

**B. sim-api serves the frontend too, from one origin**
- ✅ One app and one URL, with no CORS.
- ❌ sim-api gains static file serving and a Node build stage, so every frontend change rebuilds the Rust image.
- ❌ It deviates from README §2, and the API's routes and 404 envelope would share a namespace with static files.

**C. Cloudflare Pages**
- ✅ A CDN and preview deploys per branch.
- ❌ A second vendor, a second account, and two more CI secrets.

**Chosen: A.**

### R: what triggers a production deploy
**A. A push to `main`, after CI passes**
- ✅ `main` becomes the release branch: release by fast-forwarding `develop` into it. That also fixes the housekeeping item "`main` is behind `develop`".
- ✅ Nothing deploys without green gates.
- ❌ Releasing is a deliberate step, not automatic on every merge.

**B. Every push to `develop`**
- ✅ Always current.
- ❌ Every docs commit redeploys, and `main` stays stale.

**C. Manual `fly deploy` only**
- ✅ Nothing to set up.
- ❌ Depends on whoever has `fly` installed and logged in, and nothing guarantees the gates passed.

**Chosen: A.**

### K: keep a machine warm?
**A. `min_machines_running = 0`: machines stop when idle**
- ✅ Close to free.
- ❌ The first request after idling waits for a machine to start, about 1–2 s. The Rust binary itself starts in milliseconds.

**B. `min_machines_running = 1`**
- ✅ No cold starts.
- ❌ A small always-on cost.

**Chosen: A** for a demo. It's one line to change later.

## Deviations from README / TODO.md (CLAUDE.md requires flagging these; all approved 2026-10-03)
1. **README §2's table** gets "Fly.io (API) + Fly.io static app (frontend)" (decision F). The edit itself waits for DP7, so README keeps its current row while deploy is paused.
2. **A `rust-toolchain.toml`** pins Rust 1.98 for local builds, CI and the image. Clippy's lints change between versions, so an unpinned CI could go red without any code change.
3. **CLAUDE.md's Commands section** gains `scripts/smoke.sh`, the same way the frontend's `check` command was added there.

## Pieces
Branch `deploy`, one commit per piece. **"You run"** marks a step that needs your credentials.

| Piece | What | Depends on |
|---|---|---|
| DP1 | CI: Rust and frontend gates, plus image builds, on PRs and on pushes to `develop` and `main`. Adds `rust-toolchain.toml`. | — |
| DP2 | API image and Fly config: `Dockerfile.api`, `fly.api.toml`, `.dockerignore` | DP1 (CI builds the image) |
| DP3 | Web image and Fly config: `Dockerfile.web`, `fly.web.toml`, and the production build settings | Decision F, DP1 |
| DP4 | `scripts/smoke.sh`: the deployed API against the fixtures, CORS, and the web app | — |
| DP5 | First deploy (**you run**), then the smoke test | DP2–DP4 |
| DP6 | CD: deploy and smoke-test on a push to `main` (**you run**: create and store the deploy tokens) | DP5, decision R |
| DP7 | Docs sync | DP6 |

---

## DP1: CI (`.github/workflows/ci.yml`)
Runs on every pull request and on pushes to `develop` and `main`.

| Job | Steps |
|---|---|
| `rust` | `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`. The toolchain comes from `rust-toolchain.toml`, and the cargo cache uses `Swatinem/rust-cache`. |
| `frontend` | `npm ci` and `npm run check` in `frontend/`, with Node from `frontend/.nvmrc` |
| `images` | `docker build -f Dockerfile.api .` and `docker build -f Dockerfile.web .`, with no push, so a broken Dockerfile fails a PR rather than a deploy |

| Decision | Why |
|---|---|
| CI comes first | It closes the roadmap's "no CI" item. CLAUDE.md calls the determinism check "the one test that must never go yellow", and today it only runs locally. |
| Linux CI compares against fixtures generated on macOS | The golden-fixture test then becomes a **cross-platform determinism check** for free. |
| Third-party actions are pinned to a commit SHA | Supply-chain hygiene for a workflow that will hold deploy tokens. |

## DP2: the API image and Fly config
- **`Dockerfile.api`** is a two-stage build:
  1. `rust:1.98-slim-bookworm` runs `cargo build --release --locked -p sim-api`.
  2. The binary goes onto `gcr.io/distroless/cc-debian12:nonroot`: no shell, and not running as root.

  sim-api links no OpenSSL, because Fly terminates TLS.
- **`fly.api.toml`** (app `openrail-api`; Fly app names are global, so pick another if it's taken):
  - `PORT = 8080`, and `ALLOWED_ORIGIN` set to the web app's origin;
  - `[http_service]` on port 8080 with `force_https`, auto-stop and auto-start, and `min_machines_running` per K;
  - a request concurrency limit (soft 20, hard 40);
  - a health check on `GET /health`;
  - `kill_signal = "SIGTERM"` and `kill_timeout = "15s"`;
  - VM `shared-cpu-1x` with 256 MB;
  - region `sea`, the closest to Vancouver. Confirm at `fly launch`.
- **`.dockerignore`:** `target/`, `**/node_modules/`, `.git/` and `specs/`.

| Decision | Why |
|---|---|
| The builder and runtime images are pinned to the same Debian release (bookworm / debian12) | A newer builder glibc than the runtime image's makes the binary fail at startup. |
| `kill_timeout` (15 s) is over `REQUEST_TIMEOUT` (10 s) | Graceful shutdown can finish any in-flight request before Fly kills the machine. |
| `ALLOWED_ORIGIN` goes in `[env]`, not secrets | An origin isn't sensitive, and keeping it in the file keeps it reviewable. |

## DP3: the web image and Fly config
- **`Dockerfile.web`:**
  1. `node:26-alpine` (matching `.nvmrc`) runs `npm ci` and `npm run build`. The one build argument is `VITE_API_BASE_URL`. The client always talks HTTP; there's no `VITE_SIM_CLIENT` switch, because there's no fixtures mode (`v1-frontend-tasks.md`).
  2. The output goes onto `nginx:1-alpine`, using its default config.
- **`fly.web.toml`** (app `openrail-web`):
  - `[build.args] VITE_API_BASE_URL = "https://openrail-api.fly.dev"`;
  - port 80, `force_https`, auto-stop;
  - a health check on `GET /`.
- No rewrites are needed: replay links live in the URL fragment (`v1-frontend-tasks.md`), so every path is `/`.

| Decision | Why |
|---|---|
| The API URL is baked in at build time | Vite inlines `VITE_*` variables, and a static site has no runtime config. Changing the API's URL means rebuilding the web image, which CD does anyway. |
| nginx defaults | Vite's hashed asset names already make caching safe. Tuning cache headers is YAGNI for V1. |

Until frontend steps 6–7 land, this deploys what's done (steps 1–5). That's enough to prove the pipeline and CORS. Without `VITE_API_BASE_URL` the app calls `/api`, which only the Vite dev proxy serves, so the production build must set it.

## DP4: `scripts/smoke.sh API_URL [WEB_URL]`
It uses bash, `curl` and `jq`, and exits non-zero on the first failure.
1. **`GET /health`** returns `{"status":"ok"}`. The first request retries a few times, to ride out a cold start.
2. **`GET /scenarios`** equals `fixtures/scenarios.json`, compared as JSON with sorted keys.
3. **`POST /run`** with `charge-retry`'s story plan under naive equals `fixtures/run-charge-retry-naive.json`. **This is the cross-platform, release-build determinism check:** a link made anywhere verifies in production.
4. **`GET /replay/{that link}`** returns the same `trace_hash`.
5. **A replay link near the 100-fault cap** goes through Fly's proxy, so a long path isn't rejected in front of the app.
6. **With a `WEB_URL`:**
   - a CORS preflight from that origin is allowed;
   - `GET WEB_URL/` serves the app's HTML.

Tested locally against `cargo run -p sim-api`.

## DP5: first deploy
**You run** these, since they create apps and log in:
1. `brew install flyctl`, then `fly auth login`.
2. `fly apps create openrail-api` and `fly apps create openrail-web`.
3. `fly deploy -c fly.api.toml --remote-only`, then `fly deploy -c fly.web.toml --remote-only`.

Then I run `scripts/smoke.sh https://openrail-api.fly.dev https://openrail-web.fly.dev`, and check `fly logs` for 5xx lines.

**Rollback:** `fly releases`, then `fly deploy --image <the previous release's image>`, per app.

## DP6: CD
- **In `ci.yml`, a `deploy` job** runs only for pushes to `main`, and needs all three CI jobs to pass:
  1. `flyctl deploy` for the API, then for the web app, both `--remote-only`;
  2. then `scripts/smoke.sh`.

  It uses a `concurrency: production` group, so two deploys never overlap.
- **Secrets — you run these:**
  1. Create one deploy token per app, scoped to that app: `fly tokens create deploy -a <app>`.
  2. Store each as a repo secret: `gh secret set FLY_API_TOKEN_API` and `gh secret set FLY_API_TOKEN_WEB`.

  An app-scoped token can only deploy its own app.
- **Releasing:** fast-forward `develop` into `main`, either through a PR or with `git push origin develop:main`. Suggestion for the repo's admin: require CI to pass on `main`.

## DP7: docs sync
- **README §2:** the deploy row (F) and the CI row (DP1).
- **`v1-mvp-plan.md`:**
  - S5 done;
  - "no CI" and "`main` behind `develop`" removed from housekeeping;
  - V1 acceptance points at the deployed URL.
- **`v1-frontend-tasks.md`:** step 8's "deploy as a static site" points at `fly.web.toml` and `scripts/smoke.sh`.
- **CLAUDE.md:** `scripts/smoke.sh` in Commands (deviation 3).

## What stays after this plan
V1's acceptance (`v1-mvp-plan.md`, "Verification") is a walkthrough of the **finished UI** on the deployed URL. That needs frontend steps 1–7 (1–5 are done), plus step 8, which is this plan's smoke test. This plan makes it a push to `main` once those steps land.

## Files
- New: `.github/workflows/ci.yml`, `rust-toolchain.toml`, `Dockerfile.api`, `Dockerfile.web`, `.dockerignore`, `fly.api.toml`, `fly.web.toml` and `scripts/smoke.sh`.
- Modify (DP7): `README.md`, `CLAUDE.md`, `specs/v1-mvp-plan.md`, `specs/v1-frontend-tasks.md`.
- No Rust or frontend source changes.

## Verification
- CI is green on the `deploy` branch's PR, including both image builds.
- `scripts/smoke.sh` passes against a local `cargo run -p sim-api`, then against both deployed apps.
- After DP6: a fast-forward of `develop` into `main` deploys both apps, and the workflow's smoke step passes.
