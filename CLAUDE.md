# CLAUDE.md — Project instructions for Claude Code

## Failover & Resilience Guard (MANDATORY)

Every code change MUST be evaluated for its impact on failover, blue-green deployment, and replication BEFORE being made. This is non-negotiable.

Before committing ANY change, ask:
1. Will this break PostgreSQL streaming replication? (port bindings, pg_hba, connection strings)
2. Will this survive a container recreation? (runtime-only changes vs init scripts/volumes)
3. Will this break the blue-green swap flow? (standby-first deploy, fencing, promotion)
4. Will this break the failover engine? (quorum voting, health monitoring, WireGuard connectivity)
5. Will this affect any customer-facing service during a site failover?

If the answer to ANY of these is yes — either find an alternative approach that preserves resilience, or do not make the change. There are no exceptions.


## Version Bumping (REQUIRED on every code change)

Every commit that changes application code MUST include a version bump. Use semantic versioning (MAJOR.MINOR.PATCH). Bump PATCH for fixes/tweaks, MINOR for new features, MAJOR for breaking changes.

Update the version in ALL of these — **5 files, 6 locations**:

1. `README.md` — line near top: `**Version X.Y.Z**`
2. `frontend/package.json` — `"version": "X.Y.Z"`
3. `backend/app/main.py` — `version="X.Y.Z"` in the FastAPI() call
4. `frontend/src/components/Layout/AppShell.tsx` — `vX.Y.Z` appears **TWICE**:
   the desktop sidebar (~line 121) **and** the mobile drawer (~line 394). This
   file said "the navbar footer", singular, for a long time and the mobile one
   was repeatedly missed.
5. `flight-parser/Cargo.toml` — bump **only when the Rust parser changes.**
   `flight-parser/src/main.rs` reports `env!("CARGO_PKG_VERSION")` on
   `GET /health`, and that endpoint is the **only** reliable confirmation that a
   parser deploy actually landed — so an un-bumped parser is an unverifiable one.

Verify each line number against the current file before editing; they drift.

Include the version tag in the commit message (e.g. `— v1.7.8`).

## Tech Stack

- **Backend**: Python / FastAPI, SQLAlchemy, PostgreSQL
- **Flight log parsing**: separate Rust service (`flight-parser/`), HTTP on
  port 8100, `GET /health` reports its own crate version
- **Extended log data**: `flight_details` (1:1 sidecar) + `flight_series`
  (full-resolution time series), live since 2.82.0 — ADR-0043. **The
  `/flight-library` list query must never reference either table**; both are
  `lazy="noload"` and there are tests that fail if the compiled list SQL touches
  them (the ADR-0019 heavy-column rule)
- **Frontend**: React / TypeScript, Mantine UI, Vite
- **Infrastructure**: Docker Compose (self-hosted)
- **Deploy**: NOC Master Control fleet deployer, on push to `main` (ADR-0018). There is no in-repo deploy script — `update.sh` was deleted in `e4610b5`.

## Branch Workflow (REQUIRED)

All commits go directly to **`main`**. There is no `claude/dev` branch anymore — commit, push, deploy. No promotion step, no dev/prod split.

**There are no `./update.sh` commands — that script does not exist.** It was
deleted in `e4610b5`; the lines that documented it here were stale until
2026-09-11. A push to `main` **is** the deploy (ADR-0018): the fleet deployer
polls `origin/main`, rebuilds the changed services on BOS-HQ and recreates them.

- **Treat any push to `main` as a production deploy.** Branch and let the
  operator merge unless told otherwise.
- **Verify a deploy by what is running, never by the deployer's success line:**
  `curl -s https://droneops.barnardhq.com/openapi.json | jq -r .info.version`
  for the app, and the parser's `GET /health` → `version` for the Rust service.
- A commit whose subject carries `[skip-deploy]` is pull-only. The gate is
  **all** commits in the pushed range, so a docs commit on top of code commits
  does *not* suppress the deploy.
- `flight-parser` was missing from the deployer's `build_map` until 2026-09-06,
  so parser changes reached production only incidentally. It is mapped now.

## Decision-Making (REQUIRED)

**Before proposing or making any major architectural change** (removing features, dropping platforms, decommissioning endpoints, changing core workflows), ALWAYS:
1. Research the full context — read related code, git history, and understand WHY the feature exists
2. Identify who/what depends on it and what breaks if it's removed
3. Present findings and trade-offs to the user BEFORE taking action
4. Do NOT remove working functionality just to "simplify" — if it works, keep it unless there's a clear reason to cut it

**Example of what NOT to do:** The DroneOpsSync device upload API was decommissioned in v2.30.0 without fully considering that the browser file picker cannot access DJI app folders on Android/RC Pro. The companion app approach was correct and should not have been removed.

## Logging & Troubleshooting (REQUIRED)

Every new feature or change MUST include logging and troubleshooting support, especially for:

- **Networking / API communication** — Log request/response status, errors, timeouts, and connection failures. On the frontend, failed API calls should show clear user-facing error messages (not silent failures or blank reloads).
- **Authentication & authorization flows** — Log login attempts, token refresh failures, lockouts, and password resets with enough context to diagnose issues from logs alone.
- **Complex operations** — Database migrations, seed operations, file uploads, background tasks, external service calls — all should log start/success/failure with relevant context (IDs, durations, error details).
- **Frontend error handling** — API errors must propagate to the UI as visible notifications. Never swallow errors silently. Use try/catch with meaningful messages, not empty catch blocks.

**Minimum standards:**
- Backend: Use Python `logging` with the `doc.*` logger namespace. Include context (user, IP, entity ID) in log messages.
- Frontend: Failed API calls → Mantine notification with actionable error message. Console.error for debugging detail.
- New endpoints: Log entry, success, and failure at minimum.
- If a feature can fail in a way that's hard to diagnose remotely, add a health-check or diagnostic endpoint/log.

**Lesson learned:** The v2.38.x login lockout took multiple attempts to diagnose because the axios interceptor silently swallowed 401 errors and reloaded the page — no error was logged or shown to the user.

## Repair & Fix Quality Standard (REQUIRED)

When asked to repair or fix something, you MUST be thorough. Do not apply a surface-level patch and move on. Follow this process:

1. **Audit the full blast radius** — Find EVERY file, config, environment variable, Docker setting, and dependency that touches the broken system. Use grep/search across the entire codebase, not just the obvious files.
2. **Research best practices** — Look at what others have done that works. Check for known incompatibilities (e.g. passlib + bcrypt >= 4.0). Don't just fix symptoms — fix root causes.
3. **Check for secondary failures** — After making a fix, trace through what happens on container rebuild, server restart, database migration, and fresh deploy. If the fix only works until the next restart, it's not a fix.
4. **Verify defaults and environment** — Check config defaults, docker-compose.yml env vars, .env files. A config that defaults to the wrong value will silently undo your fix on every restart.
5. **Test the roundtrip** — If you change how data is written (e.g. password hashing), verify it can also be READ back correctly before committing. Don't assume it works.
6. **Log everything** — Every fix must include logging that would make the NEXT failure diagnosable from logs alone, without needing another debugging session.

**Lesson learned:** The v2.38.6 auth rebuild replaced passlib with direct bcrypt (correct fix) but missed that `reset_admin_password` defaulted to `True` in config.py, causing every container restart to overwrite the admin password. The fix only lasted until the next deploy.

## Tests & CI (read this before claiming anything is green)

**There is no pytest job and no cargo job in CI.** `.github/workflows/` holds
only `auto-merge-claude.yml`, `secret-scan.yml` and `self-hosted-smoke-test.yml`.
Nothing runs the test suites at merge time. Every "tests pass" claim in this
repo's history is a **local, hand-quoted** number with nothing enforcing it — so
run them yourself and **quote the real output** in the commit body or
`PROGRESS.md`. An exit code is not evidence.

```bash
cd backend && pytest -q            # expect "<N> passed, 17 skipped"
cd flight-parser && cargo test     # expect "test result: ok."
```

Three environment facts that will otherwise waste your time:

- **`aiosqlite` is required.** Seven test modules build engines on
  `sqlite+aiosqlite://`. It is pinned in `backend/requirements-dev.txt` — it was
  missing entirely until 2026-09-11, and without it those modules fail at
  *setup*, so pytest reports **ERROR, not FAIL**, and the summary still looks
  plausible: `724 passed, 29 errors` instead of `753 passed`. If you see a pass
  count ~29 short, check this first.
- **Blank the OTLP endpoint.** `app/observability/otel.py` falls back to
  `http://alloy.barnardhq.com:4317` whenever `OTEL_EXPORTER_OTLP_ENDPOINT` is
  *unset* (the skip gate only fires when it is set-but-empty). Run tests with
  `OTEL_EXPORTER_OTLP_ENDPOINT=""` or a local suite ships real spans into fleet
  observability and buries the pytest summary in exporter errors.
- **WeasyPrint needs native libs.** In a clean container, install the
  `apt-get` list from `backend/Dockerfile` (pango, cairo, gdk-pixbuf, geos,
  proj, …) or seven report modules fail collection on `libgobject`.

## Conventions

- Commit messages: short summary, optional detail paragraph, always end with session link
- Keep UI styling consistent: dark theme, cyan accents, Bebas Neue headings, Share Tech Mono for data


# Documentation Discipline (MANDATORY)

Any code change, feature change, or forward-looking plan MUST be recorded in a logical place in this repo as part of the same change. The goal: any future session can pick up exactly where the last one left off.

- **Code/feature changes** → update `CHANGELOG.md` (or equivalent) with date + summary.
- **Future plans / roadmap items** → add to `ROADMAP.md`, `docs/plans/`, or an ADR under `docs/adr/`.
- **Non-obvious decisions** → record as an ADR (`docs/adr/NNNN-title.md`).
- **Progress on in-flight work** → update `PROGRESS.md` or the relevant plan doc.

No undocumented changes. If a repo lacks the right file, create it. Commit the docs alongside the code — never in a separate follow-up that might get forgotten.

## Deployment topology (as of 2026-04-20)

Current primary host: **BOS-HQ** (Hostinger Boston, 10.99.0.4, Swarm worker).

Migrated from droneops during the 2026-04-20 6-stack migration night. NOC config.yml is authoritative; see `~/noc-master/docs/incidents/2026-04-20-helix-hub-dry-run/observations.md` for migration forensics.

PG streaming: promoted standby (droneops-standby-db) is now primary. Compose override neutralizes duplicate 'db' service and routes backend/worker/flight-parser DATABASE_URL to droneops-standby-db:5432. CHAD-HQ is failback standby.

**The BOS override is versioned** (audit 2026-06-11 P3-2): the live mechanism is
`~/droneops/docker-compose.override.yml` on BOS-HQ (auto-loaded, inlines the real
DB credential), and `docker-compose.bos-prod.yml` in this repo is the reviewed
secret-free source of truth (credential via `${BOS_PROMOTED_DATABASE_URL}` from
the host `.env`). Keep them in sync — the drift-check command is in the file
header. **Host port map:** base `db` 5434 (neutralized on BOS) ·
`droneops-standby-db` 5434 (the real primary) · demo-standby 5437. If the
override is ever absent, base `db` tries to bind 5434 and collides with the
running primary → stack bring-up fails. That's the foot-gun this guards.

Deployer: managed by NOC Master Control (`~/noc-master`).

**`.deployer-disabled` in the repo root does NOT mean "no auto-deploy."** Nothing
in the fleet deployer reads that marker; it disabled the *retired per-repo
autopull*, which no longer exists. This repo **is** continuously deployed on push
to `main`. The real way to pause deploys is
`noc-master/data/soak-pause/<repo>.pause`.

**Public hostnames:** prod is **`https://droneops.barnardhq.com`**. Despite
older docs, `command.barnardhq.com` has **no DNS record** — it resolves only
via the zone wildcard and returns CF 530 (ADR-0010 documents the correction).
Demo is `https://command-demo.barnardhq.com`.

**Demo stack (updated 2026-06-11):** separate clone `~/droneops-demo` on
BOS-HQ, **NOT deployer-managed** — the NOC deployer only targets prod, so the
demo must be updated manually:
`cd ~/droneops-demo && git pull --ff-only && docker compose -p droneops-demo
-f docker-compose.yml -f docker-compose.demo.yml --env-file .env.demo up -d --build`.
Known recreate gotchas (all hit on the 2026-06-11 v2.70.1 update — see
`docs/incidents/2026-06-11-deploy-rename-conflicts-demo-bringup.md`):
containers created outside compose carry no compose labels and cause name
conflicts on recreate (`docker rm -f` them first), and a recreated cloudflared
re-reads `.env.demo` — if the tunnel token rotated since the container was
created, registration fails with "Invalid tunnel secret" (fix per
`docs/cloudflare-tunnel-setup.md` troubleshooting).

## Notifications (ADR-0036 + ADR-0006 addendum, 2026-04-26)

**Transport:** self-hosted ntfy at `https://ntfy.barnardhq.com` (BOS-HQ).
Migrated from Pushover. Same dedup + Redis suppression + `send_alert` /
`send_alert_sync` signatures preserved verbatim — only the transport
changed. See `docs/adr/0006-pushover-to-ntfy-migration-addendum.md`.

**Module:** `backend/app/services/ntfy.py` (replaces `pushover.py`).
Same public API consumed by `backend/app/auth/device.py:26` and
`backend/app/routers/admin_device_rotation.py:35,175,177`.

**Env var:** `NTFY_DRONEOPS_PUBLISHER_TOKEN` (replaces `PUSHOVER_TOKEN` +
`PUSHOVER_USER_KEY`). Single token. Set in BOS-HQ `~/droneops/.env`.

**Watchdog contract preserved:**
- ADR-0002 §5 layers 3+4 still page on device silence > 48h and on
  first-401 device-key auth failure.
- Redis dedup key prefix kept verbatim (`doc:pushover:dedup:`) so
  in-flight dedup entries continued working through cutover. Renaming
  it later is a separate cosmetic decom.
- Fail-soft: missing token → log only (no exception). Same as before.
- Publisher fallback to `ntfy.sh` on per-service obscured topic when
  the primary is unreachable; `[FALLBACK]` title prefix.

**Click URL:** ntfy alerts include `https://noc-mastercontrol.barnardhq.com/status/droneops` as the click-fallback target (NOT `noc.barnardhq.com` — that's InfraWatch's dashboard).

## Verify before you advocate (fleet-wide, ADR-0181)

A capable model is not a correct one — fluency and confidence are not evidence.
The default posture on every non-trivial call is **check, then act**, never _act,
then hope_.

1. **Primary source over memory.** Read the actual file, run the actual query,
   hit the actual API, `docker exec` the actual container. Memories, prior
   sessions, ADRs and inherited copy are point-in-time claims — stale until
   re-verified. Docs describe intent; the running system is truth.
2. **Best-practice path, not merely one that runs.** If the approach is
   non-obvious, research it — upstream docs, existing fleet precedent and ADRs,
   the tool's own help — before committing to it. "It works" is not the bar.
3. **State the reasoning, not just the conclusion.** If the logic doesn't
   survive being written down, it wasn't sound.
4. **Verify the observable end state after acting.** Not the exit code: the
   version actually live, the row actually present, the alert actually
   delivered, the suite green with its output quoted.
5. **Say when uncertain.** "I believe X but haven't confirmed Y" beats false
   confidence. State assumptions at the point you make them.

**Red flags — each means stop and verify:** "this should work", "it's probably
still configured that way", "I remember this file does X", "the docs say so",
"that's how it usually works"; reaching for a fix before reading the code;
feeling certain about a system you haven't actually looked at this session.

**Not a licence for paralysis**, for producing a runbook instead of doing the
work, or for asking a question that reading a file would answer. Verify, then
execute — the check is a step inside the work, not a substitute for it.

Full reasoning, prior art (ADR-0178, ADR-0180) and alternatives considered:
`noc-master/docs/adr/0181-verify-before-advocating.md`. Also carried in the
operator's global `~/.claude/CLAUDE.md`; this copy travels with the repo.
