# DroneOpsCommand — In-Flight Work

Maintained alongside `CHANGELOG.md` and `docs/adr/`. `CHANGELOG.md` is
the ledger of shipped changes; this file tracks what's in-flight or
blocked.

## 2026-09-11 — FP-1 **P-EVAL complete**: no crate bump exists; P2 is unblocked on `0.5.7`

**State: EVIDENCE DELIVERED, awaiting Bill's call on §8's follow-ups.** Nothing
was adopted and nothing was deployed. Report:
`docs/reports/2026-09-11-dji-log-parser-upgrade-eval.md`. Harness: `tools/p-eval/`.

ADR-0043's **D6** asked for a before/after diff before the crate moves. The
finding is that **the newest published `dji-log-parser` is `0.5.7` — already the
pinned version.** Two independent sources agree (crates.io API; `cargo search`
inside `rust:1.85-bookworm`). Upstream last released 2025-04-26 and is one commit
ahead of that tag, dated 2025-06-07.

That one commit ("Parse Inspire 1 battery serial numbers", `88fcfc96`) was
evaluated as the candidate because it is the only candidate that exists.

| Measurement | Result |
|---|---|
| Comparisons run | **782** (198 live originals + 584 recovered; 20 overlap) |
| Distinct real DJI logs | **762**, all log **v14** |
| Metrics diffed per log | **24**, floats compared **bitwise** (`to_bits`), not by tolerance |
| `gps_track` coordinates compared | **6,756,743** (full-precision ordered SHA-256 digest per track) |
| Frames decoded | **6,797,600**; `frames_decoded` true for 782/782 |
| DJI keychains fetched / failed | **759 / 0** (23 cache hits = 3 pilot + the 20 overlaps) |
| Parse errors | **0** |
| **Logs with ANY difference** | **0** |
| Runtime | 1202 s on 5 cores, off-prod (logs copied to the workspace; BOS untouched) |

**The zero is falsified, not assumed.** `p-eval --selftest` drives the candidate's
only changed function to a known-different answer (`Inspire1`: `"0987654321"` →
`"1234567890"`), proves the change is reachable *only* via the three Inspire-1
product types, and proves the comparator reports a one-ULP float difference.
`selftest failures: 0`. A corpus with no Inspire 1 is therefore *predicted* to
show zero differences.

**Second control — the harness against production's own rows**, partitioned by
when each row was written:

| Row era | duration_secs | total_distance |
|---|---:|---:|
| Written by today's parser (73 rows) | **73 / 73** | **73 / 73** |
| Legacy, pre-ADR-0027/0028 (125 rows) | 62 / 125 | 123 / 125 |

`max_altitude`, `max_speed`, `point_count`, `frame_count`, `product_type`:
**198 / 198** each. Every disagreement is a legacy row with a named cause —
ADR-0027 (duration source) for 63, ADR-0028's C1 outlier gate for 2 (the harness
drops one teleport segment those rows predate), ADR-0044's matcher rewriting
`drone_model` to the canonical fleet name for 129.

**Quoted test output (no CI job runs these):**

```
tools/p-eval  cargo test → test result: ok. 13 passed; 0 failed; 0 ignored
tools/p-eval  --selftest → selftest failures: 0
flight-parser cargo test → test result: ok. 65 passed; 0 failed; 0 ignored
```

### What P2 inherits

- **Build the §2.4 `SmartBatteryStatic` shim** — upstream has not fixed it
  (`record/smart_battery_group.rs` is identical between pin and candidate). The
  plan's `raw >> 8` is the right transform; the new constraint is that it
  recovers the true value only while the top byte is zero, so **`loop_times`
  silently wraps at 256 cycles** and the `0..=3000` plausibility gate cannot
  catch it. Confirm in P2-a against a pack with a three-digit cycle count.
- **Keep the `Unknown(NNN) → aircraft_name` fallback in full.** Four placeholders
  live, not three: `Unknown(150)` (Matrice 4T) exists on 39 recovered logs and no
  native row yet, so P7's dry-run should expect four values from P4(b)'s
  `^Unknown\(\d+\)$` predicate.
- **Peak RSS is still unmeasured** against the parser's 256 MB `mem_limit`. The
  harness is not a proxy — it holds two frame vectors and two track copies, so its
  footprint is structurally larger. Still P2-a's gate.

### Open for Bill

1. **Pin the crate exactly** (`= "0.5.7"`). `Cargo.toml` currently requests
   `"0.5"`, so a `cargo update` could move it and bypass D6 without a decision.
   Not done here — it is a change to the manifest D6 governs.
2. **Guard `DJI_LOG_PARSER_VERSION`** against `Cargo.lock`. It is stamped on every
   `flight_details` row as the provenance D6's "re-backfill below version X" query
   depends on, and nothing keeps it honest.
3. **153 of 226 `dji_txt` rows are legacy** and carry pre-ADR-0027/0028 duration
   and distance. D5 freezes those fields, so they stay stale unless something
   reprocesses. The error is small — 6 rows off by more than 1 s, +35.7 s total
   across the 125 measured — and leaving it is defensible. It should be a
   decision, not an oversight.
4. **Operational note:** the DJI decode key is **not** in the parser's
   environment anywhere (prod and demo both have `DJI_API_KEY=""`, so
   `GET /health` → `dji_key_configured: false` is correct by design, not a
   defect). Production reads it from `system_settings.dji_api_key` and passes it
   per-request as `X-DJI-Api-Key`. Worth knowing before anyone "fixes" that
   health field.

## 2026-09-05 — Fleet-attribution matcher: canonical DJI serials (ADR-0044) — **LIVE IN PRODUCTION**

**State: MERGED AND LIVE.** Operator gave the go on 2026-09-05.
`feat/serial-prefix-matcher` was fast-forwarded onto `main`
(`34553cf` → **`dfc7054`**) and pushed; the fleet deployer built and
recreated the stack. Version **2.90.0**, deliberately clear of the
2.84.0–2.89.0 band FP-1 reserves for P2–P7.

**Post-deploy verification, read off the running system (not a log line):**

| Check | Observed |
|---|---|
| `openapi.json` → `info.version` | **2.90.0** |
| flight-parser `GET /health` → `version` | 1.2.0 (unchanged) |
| alembic head | `0011_battery_src_truth` (unchanged — this change adds no migration) |
| flights with `aircraft_id IS NULL` and a serial | **0** (was 88) |
| attributed to `DJI Matrice 4TD` (`1581F8HGX255P00A`) | **50** |
| attributed to `DJI Matrice 4T` (`1581F7K3C25AA00D`) | **39** |
| aircraft rows / duplicate serials | 11 / 0 |
| backend ERROR or CRITICAL since startup | **0** |

Backend startup logged `STARTUP: Aircraft backfill — 88/88 unlinked matched`.
The 4TD total reads 50 rather than the 49 predicted because one of its ODL
rows was already attributed before this change; 49 newly linked + 39 = the 88
the log reports, so the counts reconcile exactly.

- The defect and the rule are recorded in
  `docs/adr/0044-serial-prefix-matcher-odl-canonical-serials.md`.
- Verified against the production DB on BOS-HQ before and after writing
  the rule: exactly 88 `aircraft_id IS NULL` flights, all
  `opendronelog_import`, all 20-char serials
  (`1581F8HGX255P00A0FEK` ×49, `1581F7K3C25AA00DMZMG` ×39).
- **This deployed as a bulk write, as designed.** The startup backfill runs
  on every container restart against `aircraft_id IS NULL`; it attributed all
  88 rows on the first restart after the merge. Note the corollary recorded in
  ADR-0044: the backfill will not re-evaluate a row once `aircraft_id` is set,
  so correcting any one of those 88 is now a manual detach through the UI.
- Depends on the aircraft rows Bill's earlier data work created
  (`DJI Matrice 4T` / `1581F7K3C25AA00D`, `DJI FPV` / `37Q7LA800BX0PN`);
  this change is code-only and touched no DB rows.
- **Coordination:** FP-1 P0+P1 merged first; this landed on top of it. The diff is confined to the matcher (`flight_library.py`),
  its test file, ADR-0044, docs and the version markers, and it merges
  cleanly — FP-1's `flight_library.py` edits are in the ingest, status and
  telemetry regions, not in `_match_fleet_aircraft`. ADR **0044** is free
  on `34553cf` (FP-1 stopped at 0043) and is claimed here.
- Interacts with FP-1: the planned ODL-era re-import (ADR-0043) lands
  20-char serials, which this rule is what makes attributable.

**Test evidence at the rebased HEAD — there is no pytest or cargo job in CI,
so this is local and quoted, not inferred from an exit code.** Run in
`python:3.13.5-slim-bookworm` with the backend Dockerfile's native libs and
`requirements.txt` + `requirements-dev.txt`, hermetic (no live Postgres, so
the migration integration tier skips), `OTEL_EXPORTER_OTLP_ENDPOINT=""`:

```
$ python -m pytest -q
753 passed, 17 skipped in 268.46s (0:04:28)

$ python -m pytest tests/test_flight_attribution.py -q
22 passed in 3.87s
```

Baseline on `origin/main` `34553cf`, same command and image: `743 passed,
17 skipped`. The delta is exactly the 10 cases this branch adds. The
`flight-parser` crate is untouched here and its suite is unchanged
(`rust:1.85-bookworm`, matching `flight-parser/Dockerfile`):

```
$ cargo test
test result: ok. 65 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

**Repo defect found while doing this, fixed in its own commit on this
branch:** `aiosqlite` was declared in neither `backend/requirements.txt` nor
`backend/requirements-dev.txt`, yet seven test modules build engines on
`sqlite+aiosqlite://`. It is missing on `main` too — pre-existing, not
something this branch or FP-1 introduced. A clean-room install loses those
modules at *setup*, so pytest reports ERROR rather than FAIL: measured at
this commit, `724 passed, 17 skipped, 29 errors` without it vs `753 passed,
17 skipped` with it. Now pinned `aiosqlite==0.20.0` in
`requirements-dev.txt`. Kept as a separate commit so it can be reverted
independently of the matcher change.

## 2026-09-05 — FP-1 Flight Details — P0 + P1 **LIVE IN PRODUCTION**

Operator gave the go on 2026-09-05. `feat/fp1-flight-details` was
fast-forwarded onto `main` (`9f502e0` → **`34553cf`**) and pushed at
**02:35 PDT**. A push to `main` IS a production deploy on this repo
(ADR-0018, NOC fleet deployer), so that push deployed BOS-HQ.

**Deploy verified live — see "Production verification" below for the
container-content evidence.** Both phases remain behaviourally inert as
designed: the new tables exist and are empty, and details are only written
by imports that happen from now on.

### Production verification (2026-09-05, ~02:35–02:40 PDT)

Verified by **container content and live DB**, never by a deployer log line
(the silent-stale-deploy class). Every value below was read off the running
system on BOS-HQ.

**Deployer handling of the push** — it did NOT go pull-only. The two HEAD-most
commits carry `[skip-deploy]`, but the gate is `allCommitsSkipDeploy`, and the
two code commits beneath them do not carry it, so the range deployed:

```
"from":"9f502e0a","to":"34553cf3","msg":"Changes detected — starting deploy"
"msg":"Building all services on remote..."
"changed":["backend","frontend","worker"],"msg":"Image-digest gate passed (ADR-0056)"
"msg":"Recreating changed services on remote (up -d, no down — ADR-0066)..."
"msg":"Smoke test PASSED"
"status":"success","duration":"267368ms","services_actually_rebuilt":["backend","frontend","worker"]
```

**Live artifacts, as actually served:**

| Check | Expected | Observed |
|---|---|---|
| `droneops.barnardhq.com/openapi.json` → `info.version` | 2.83.0 | **2.83.0** |
| flight-parser `GET :8100/health` → `version` | 1.2.0 | **1.2.0** |
| alembic head (`droneops-standby-db`) | `0011_battery_src_truth` | **`0011_battery_src_truth`** |
| `flight_details` table | exists | **exists, 77 columns, 0 rows** |
| `flight_series` table | exists | **exists, 7 columns, 0 rows** |
| `0011` battery columns | 3 added | **`batteries.cycle_count_observed`, `batteries.metrics_source`, `battery_logs.pack_cycle_count`** |
| read-path routes in live OpenAPI | 3 | **`/api/flight-library/{flight_id}/details`, `/details/series`, `/api/flight-library/details/status`** |

Container recreation confirmed by timestamp, not by log: `backend`,
`frontend`, `worker`, `beat` and `flight-parser` all recreated 02:38:02–02:38:03
PDT from images built 02:37:03–02:37:09 PDT.

**Migrations applied cleanly**, from the backend's own startup log — a single
forward run under the ADR-0035 advisory lock, no errors:

```
MIGRATIONS: upgrading schema from 0009_mission_dl_email_sent_at to head 0011_battery_src_truth (ADR-0022).
Running upgrade 0009_mission_dl_email_sent_at -> 0010_flight_details, ADR-0043 — flight_details + flight_series sidecar tables
Running upgrade 0010_flight_details -> 0011_battery_src_truth, ADR-0043 D4 — battery source-of-truth columns (landed early, inert)
MIGRATIONS: upgraded complete (head=0011_battery_src_truth)
```

**Prod data invariants held across the deploy** (pre-swap → post-swap):
`aircraft` 11 → **11** rows, duplicate serials 0 → **0**, and flights with
`aircraft_id IS NULL` **88 → 88, unchanged**. The startup aircraft-backfill ran
and deliberately left all 88 unattributed, logging INFO (not error) for exactly
two serials that account for the whole set:

```
39  serial=1581F7K3C25AA00DMZMG
49  serial=1581F8HGX255P00A0FEK
```

Both are the log-side **superset** form of a fleet serial (e.g. fleet
`1581F7K3C25AA00D` vs log `…00DMZMG`), which is precisely the mismatch the
matcher change addresses. **Superseded later the same day:** ADR-0044 shipped
in `dfc7054` and the startup backfill attributed all 88 — see the matcher
section above. The 88 recorded here is the state of *this* deploy
(`34553cf`), not the current state, which is 0. `grep -ci 'error|exception|traceback|critical'`
over the backend log since startup: **0**.

**One residual to fix separately — `flight-parser` is absent from the
deployer's `build_map`** for this repo (`noc-master/data/config.yml` maps only
`backend`, `frontend`, `worker`). The ADR-0056 digest gate and the
`services_actually_rebuilt` field are both computed *over `build_map`*, so the
parser is structurally invisible to them — which is why a deploy that
demonstrably rebuilt and recreated the parser still reports
`services_actually_rebuilt: ["backend","frontend","worker"]`.

The parser went live anyway because both surrounding steps are unscoped: with
no `external_build_cmd` configured the build is a bare `docker compose build`
(all services), and the recreate is a bare `up -d --remove-orphans`, which
recreates anything whose image ID changed. So the outcome was correct, but it
was correct *incidentally* — the gate that exists to catch a silent stale
parser cannot see the parser. Adding a `flight-parser` entry to `build_map`
would close that. **Not tested:** whether a parser-ONLY change (no
`backend/`/`frontend/` diff) still deploys; that path was not exercised here.

### P0 — schema + read path (v2.82.0) — LIVE, inert

Migrations `0010_flight_details` (both tables) and `0011_battery_src_truth`
(three nullable battery columns, landed a phase early so the battery
source-of-truth phase needs no migration). Models, `Flight.details` /
`Flight.series` at `lazy="noload"`, schemas, `/details`, `/details/series`,
`/details/status`, the extracted `telemetry_downsample` service, and the §4.4
report-audience guard. **Nothing writes to the new tables yet; no existing
response changes.**

**Test evidence — there is no pytest or cargo job in CI, so this is local and
quoted, not inferred from an exit code.**

Hermetic suite (`cd backend && pytest -q`):

```
723 passed, 7 skipped in 75.24s (0:01:15)
```

Full suite with a live Postgres 16, which un-skips the migration integration
tier (`docker run -d --name doc-mig-test -e POSTGRES_USER=doc -e
POSTGRES_PASSWORD=test -e POSTGRES_DB=doc -p 55432:5432 postgres:16-alpine`;
then `DOC_TEST_PG_URL=postgresql+asyncpg://doc:test@127.0.0.1:55432/doc
DATABASE_URL=$DOC_TEST_PG_URL pytest -q`):

```
729 passed, 1 skipped in 89.70s (0:01:29)
```

Baseline before this work, same command, same container, from a clean
`origin/main` export: `618 passed, 1 skipped`. So P0 adds 111 passing tests
and breaks nothing.

**A gap in the pre-existing migration tests, closed.** `test_db_migrations.py`
builds every test database with `create_all` from the LIVE models first. Since
the models now declare the new tables, that tier only ever exercises 0010/0011's
*idempotency guards* — `op.create_table` and `op.add_column` are never reached,
so a typo in either DDL body would have sat green through the whole suite and
first appeared as a BOS-HQ crash loop. `tests/test_migration_0010_0011_upgrade_path.py`
reproduces the actual production shape instead (full legacy schema, new objects
dropped, stamped at 0009) and upgrades, covering the CREATE branch, the FK
`ON DELETE CASCADE`, "primary key only, no secondary indexes", a second run
being a no-op, and an empty autogenerate diff against `Base.metadata`.

**One self-inflicted defect found and fixed during that work,** worth recording
because it is the ADR-0042 hazard biting from a new direction: the first draft
of that test stamped 0009 with `alembic.command.stamp(_alembic_config(), ...)`.
A bare `Config` has no `connection` attribute, so `alembic/env.py` takes its CLI
branch and runs `fileConfig()`, which defaults to `disable_existing_loggers=True`
and killed every `doc.*` logger for the rest of the pytest process — making
three unrelated log-assertion tests fail depending on file ordering. They passed
in isolation and failed only in the full run. Confirmed mine rather than
pre-existing by running the same command against a clean `origin/main` export
(618 passed, 0 failures). Fixed by writing the `alembic_version` row with SQL.
**Anything that calls Alembic programmatically outside `run_migrations_sync`
needs the same care.**

**§1.5 / C-2 encoding measurement — DECIDED by measurement, `json` stands.**
Run against real `postgres:16-alpine`, synthesising the census's largest flight
(M4TD, 13,870 frames) with a realistic climb/cruise/descent profile and §2.5
per-quantity rounding, each series written three ways:

| series | n | dp | raw json | `json` | `jsonb` | `float8[]` |
|---|---:|---:|---:|---:|---:|---:|
| altitude_msl_m | 13,870 | 1 | 81,187 | **23,490** | 32,656 | 24,937 |
| t_offset_s | 13,870 | 2 | 90,817 | **40,894** | 47,724 | 45,410 |
| battery_current_a | 13,870 | 2 | 77,134 | **32,707** | 42,650 | 36,415 |
| pilot_lat | 657 | 7 | 7,165 | **1,921** | 2,346 | 3,134 |
| **total** | | | 256,303 | **99,012** | 125,376 | 109,896 |

Read + parse of 13,870 samples to a Python list, best of 60:
`json` **4.34 ms**, `jsonb` 7.28 ms, `float8[]` 9.43 ms.

Conclusions, including two that correct the plan:
- `json` wins on **both** axes — 11 % smaller than `float8[]` and ~2.2x faster
  to read. The plan's §1.5 speculation that a native float array "needs no JSON
  parse in Python and maps straight to a list" and might therefore be faster is
  **wrong as measured**: psycopg2's array parser is slower than the C
  `json.loads`. C-2 is closed; `values` stays `json`.
- Compression is **2.59:1**, not the plan's assumed 3–5x. So the largest
  flight is ~500 KB compressed rather than 260–430 KB, and 210 flights land
  nearer **35–70 MB** than the plan's 25–60 MB. Same order, still comfortable,
  but the estimate should not be quoted at the optimistic end.
- Caveat on the caveat: this is synthetic data with gaussian noise, which is
  roughly a worst case for compressing digit runs. Real sensor data at 1 dp
  with slow drift should do slightly better. The measurement is a floor.
- Unexpected: `t_offset_s` is the **largest** series, bigger than altitude —
  monotonically increasing 2-dp values have high digit entropy. If storage ever
  needs trimming, storing a start + cadence instead of a full time base is the
  cheapest win available. Not done; out of scope.

Script kept at `/tmp/claude-1000/.../enc_measure.py` for the session only —
it writes nothing outside a scratch table on a throwaway container, and touched
no production database.

### P1 — Tier 0 parser pass (app v2.83.0, parser v1.2.0) — LIVE

All of §2.2 in the existing frame loop, §2.5 per-quantity rounding, full-
resolution series, `details: None` in `litchi.rs` / `airdata.rs`, the
`Unknown(NNN)` → `aircraft_name` fallback, and backend persistence inside the
existing best-effort savepoint. **No second decode and no second DJI keychain
round-trip** — the accumulator rides the loop that was already there.

Produced per DJI flight: ~50 typed scalars, five JSONB groups, and **15**
full-resolution frame series (`t_offset_s`, MSL altitude, VPS height,
distance-from-home, z-speed, RC down/uplink, gimbal P/R/Y, aircraft P/R/Y,
battery current, cell-voltage deviation).

**Test evidence — no cargo or pytest job exists in CI, so this is local and
quoted.**

`cd flight-parser && cargo test` (run in a `rust:1-bookworm` container to
escape the workspace's ~2.5 GB cgroup cap):

```
running 65 tests
...
test result: ok. 65 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.11s
```

Baseline on `origin/main` was 20 tests, so P1 adds 45.

`cd backend && pytest -q`:

```
743 passed, 17 skipped in 113.29s (0:01:53)
```

With the live Postgres 16 (un-skips both DB tiers):

```
759 passed, 1 skipped in 194.70s (0:03:14)
```

Cumulative against the `origin/main` baseline of `618 passed, 1 skipped`:
**+141 passing backend tests, +45 Rust tests.**

**The cross-language wire fixture, and why it exists.** The payload crosses a
JSON boundary between two languages, whose silent failure mode is: rename a
field on one side, every column writes NULL, and the import still logs
"Flight details stored". Permanent, invisible, and it looks exactly like
success. So `backend/tests/fixtures/parser_details_payload.json` is
**generated by the Rust suite** and asserted against by both sides — Rust
checks the checked-in copy still matches what it emits; pytest checks every key
in it maps to a real column and that a real row comes back populated.

Regenerate with:
`cd flight-parser && DETAILS_FIXTURE_OUT=../backend/tests/fixtures/parser_details_payload.json cargo test emit_wire_fixture`

Verified by falsification rather than assumed: deliberately drifting the
fixture (`photo_count` → `photoCount`) turned **three** tests red, including
`assert None == 2` on the stored row — the exact silent-NULL symptom. Fixture
restored afterwards.

**Two defects found in my own implementation during P1, both by a test:**

1. `video_seconds` attributed the interval `[prev_frame, this_frame]` to the
   *current* frame's recording flag, while the phase histogram (correctly)
   attributes it to the state that actually held during it. That shifts every
   interval by one sample and silently drops the last stretch of a recording —
   3 s reported for a 6 s clip. Both now use the same convention.
2. `waypoint_mode_seconds` serialised as `-0.0`. Rust's `Sum` impl for floats
   folds from `-0.0`, so a mode that never occurred sums to negative zero:
   numerically fine, but it reads as a defect on screen and churns every JSON
   diff. `round_dp` now normalises it, with a regression test that asserts the
   std-library premise it depends on.

**Decisions taken during P1 that the plan did not specify:**

- **Missing samples are JSON `null`, not `0.0`.** Series values are
  `Option<f64>`. An RC link with no OFDM record yet, or a distance-from-home
  with no GPS fix, stores a gap — a `0` there would read as "signal lost" or
  "at the home point", which are different and alarming claims about the
  flight. Costs ~4 bytes per genuinely-absent sample.
- **Time-base provenance is recorded.** `FrameCustom::default()` is the Unix
  epoch, so a log with no `Custom` records would otherwise stamp 1970 on every
  sample. The base is chosen explicitly (wall clock → `fly_time` → none) and
  written to `config.time_base`; with no base, `t_offset_s` is omitted rather
  than filled with zeros, and `first/last_frame_at` and `frame_hz_est` stay
  NULL.
- **Takeoff/landing come from `osd.is_on_ground` edges**, a direct physical
  signal, rather than from inferring intent from mode names.
- **The crate injects its own `"Flight mode changed to X."` into `app.tip`.**
  We emit structured `kind: "mode"` events from `flyc_state`, so the textual
  duplicate is dropped — otherwise every transition appears twice.
- **`battery_energy_wh` / `battery_discharge_mah` integrate over the real
  inter-frame interval**, making them invariant to log rate. A
  frame-count-times-assumed-cadence integration is the ADR-0027 mistake in a
  new place; there is a test that logs the same flight at 1 Hz and 5 Hz and
  requires the same answer.

**Response-size impact, checked rather than assumed:** every call site posts
exactly ONE file to `/parse` (`files={"file": ...}` in `_SpooledUpload.parse`
and in the Celery device-upload task), so the ~1.3 MB details payload is
per-request and is not multiplied by a batch upload.

**NOT measured:** peak parser RSS on a real 13,870-frame log. Arithmetic bounds
it at roughly 3–4 MB of series buffers plus the serialised JSON, well inside
the container's `mem_limit: 256m`, but there is no local DJI original to run it
against and I have not exercised the real path end to end. The plan's R-2 memory
gate belongs to the Tier 1 phase and should cover this too.

**Also not done in this run** (correctly out of scope): the Tier 1 record pass,
the crate before/after evaluation, backfill, repair, the UI, the battery
source-of-truth switch, and the ODL re-import. The crate evaluation runs
**before** the Tier 1 phase by design — if a newer crate fixes
`SmartBatteryStatic` and `ProductType`, part of that phase shrinks or vanishes.

### Corrections to the plan from live prod data (2026-09-05)

Found by a parallel session verifying the matcher contract against the BOS-HQ
prod DB. **Note the prod DB is `droneops-standby-db` (db `droneops`), the
promoted standby — NOT `droneops-db-1`, which is an `alpine:3` placeholder.**

1. **DJI serials exist in two forms and the plan conflates them.** Every
   OpenDroneLog-era `drone_serial` is the 16-char header serial plus a 4-char
   suffix — e.g. `1581F8HGX255P00A` + `0FEK` (Matrice 4TD),
   `1581F5BK7241J00B` + `A040` (M30T). The 14-char FPV serials carry no suffix
   and are identical in both forms. **§7.1 as written matches zero of the 584
   files**, because it requires the parsed 16-char header serial to be *equal*
   to the stored 20-char row value. §7.1 now states the serial check is a
   **16-char-prefix comparison**, not equality. The plan file is corrected on
   this branch.
2. **The "88 unattributed ODL rows" have a different cause than the plan
   states.** Verified: 49 Matrice 4TD + 39 Matrice 4T. The 4TD's aircraft row
   has existed since 2026-03-16 with serial `1581F8HGX255P00A`; those 49 are
   unattributed purely because of the 16-vs-20-char mismatch, not a missing
   row. The plan's claim that the two missing aircraft rows account for 46
   unattributed flights is wrong.
3. **Relevant to P1:** the parser stamps `drone_serial` from the DJI log
   header (`details.aircraft_sn`, 16 bytes for log version > 5), so newly
   imported flights carry the **16-char** form. `_match_fleet_aircraft`
   (`flight_library.py` branch 1) does `func.upper(Aircraft.serial_number) ==
   drone_serial.upper()` via `scalar_one_or_none()` — **exact equality, no
   prefix logic.** P0/P1 deliberately do NOT change the matcher and build
   nothing that assumes the two forms interoperate. The 20-char form is where
   `flight_details.aircraft_sn_full` will land in the Tier-1 phase, which is
   the natural place to reconcile them later.

### Production data changes applied 2026-09-05 (by the parallel session, not by this branch)

All on the BOS-HQ prod DB, verified by reading the rows back:

1. **Created** aircraft `DJI Matrice 4T`, serial `1581F7K3C25AA00D` (16-char
   header form), `image_filename` NULL — **there is no `dji_m4t_official.png`
   in `/app/app/static/aircraft/`; one is needed before the fleet tile renders
   properly.**
2. **Created** aircraft `DJI FPV`, serial `37Q7LA800BX0PN`, image
   `dji_fpv_official.png`.
3. **Renamed** the pre-existing row with serial `37QBJ5WBD100DN` from
   `DJI FPV` to `DJI FPV - DECOM` (ODL's own label is "DJI FPV (DECOM)"; also
   required so branch 2 of `_match_fleet_aircraft`, the no-serial model
   fallback, does not go ambiguous on two identically-named rows).
4. **Re-pointed 9 flights** with `drone_serial = '37Q7LA800BX0PN'` from the
   DECOM airframe (`f09558d1-…`) to the new active row
   (`007e1483-e5e3-45f9-9610-378f61f5523d`) — they had been fuzzy-matched to
   the wrong airframe pre-ADR-0007. 7 `opendronelog_import`, 2 `dji_txt`.
   Blast radius verified nil first: 0 `mission_flights`, 0
   `maintenance_records`, 0 `maintenance_schedules`, 0 `batteries` referenced
   the DECOM row; the 9 `battery_logs` derive their airframe through
   `Flight.aircraft_id` and follow automatically. Post-assert inside the
   transaction required exactly 9 moved / 0 stragglers. Final: DECOM 3
   flights, active 9.

Also filled `specs` on the Matrice 4T from DJI's published enterprise page.
**The M4T is not IP-rated and its figures legitimately differ from the M4TD**
(49 vs 54 min, 1219 g vs 1850 g, 6000 m vs 6500 m ceiling, no QZSS) — the 4TD
is the heavier dock-compatible variant. A future reader should not "correct"
one to match the other.

Consequences: the startup backfill in `main.py` only touches
`aircraft_id IS NULL`, so none of the above shifts on the next container
restart, and the new M4T row will **not** pick up the 39 existing ODL rows
(20-char serial mismatch, per correction 1). Aircraft table is now 11 rows,
zero duplicate serials.

### Log inventory — corrected 2026-09-05 (recovery hunt finished)

**Counts re-verified by me against the live prod DB on 2026-09-05, not taken
from the plan:**

```
$ docker exec droneops-standby-db psql -U droneops -d droneops -tAc \
    "SELECT source, count(*) FROM flights GROUP BY source ORDER BY 2 DESC"
opendronelog_import|584
dji_txt|218
$ docker exec droneops-backend-1 sh -c 'ls /data/uploads/flight_logs | wc -l'
192
```

So: `dji_txt` is **218** (not 210 — 8 uploaded 2026-09-04 23:47 PDT), retained
files **192** (not 184), of which 2 are dummy test files → **190 real
originals**. The S3 mirror holds 184, not 183. **The 28 file-less rows are
unchanged.** Anything in the plan that hard-codes 210 or 182 is stale by
construction — the backfill and the crate evaluation should re-derive the
count at run time, because it moves every time Bill uploads.

- **All 584 OpenDroneLog-era originals recovered** from Bill's Google Drive to
  BOS-HQ `~/droneops-staging/drive-logs/` (0 download failures; sha256 + header
  CSV alongside; `docs/plans/data/2026-09-04-drive-logs-inventory.csv`).
  584/584 filenames match the `opendronelog_import` rows 1:1 → P7 matches on
  `original_filename`. 548 hashes equal ODL's own recorded sha256. 20 duplicate
  existing `dji_txt` flights; 564 new.
- **The staging backup gap is CLOSED** (2026-09-05): those 584 files are in the
  existing droneops restic repo under tag `staging` — snapshot `4c08afa7`,
  2.181 GiB, in R2, independently verified. Source mounted read-only; nothing
  moved or reconfigured, so there is nothing for the deployer to clobber.
  **Caveat: this is a one-shot snapshot of a static archive, not a recurring
  lane.** Files added after 2026-09-05 are unprotected; the durable fix is P7
  ingesting them into `/data/uploads/flight_logs/`.
- **The 28 missing `dji_txt` originals are unrecoverable, and the mechanism is
  now proven rather than inferred.** Of the 52 rows created 2026-03-23 →
  2026-04-19, exactly 24 have files, and that set of 24 is byte-identical to
  `~/migration/doc_appdata.tar.gz`. **The survival boundary is not a date — it
  is "was the file inside the migration tarball."** HSH's `~/backups/pg-backup.sh`
  still exists and runs only `pg_dump` plus an n8n sqlite `.backup`; it never
  touched `/data/uploads`. The HSH prod era had **no file-level backup of
  flight logs at all** — the bytes were never captured. This is firmer than the
  plan's "backups began 2026-07-16" and it closes the question.
- Their `original_filename` values are recovered and follow **three distinct
  naming patterns**, so a search for `DJIFlightRecord*` alone misses 12 of 28.
  Manifest (`missing_28_full.tsv`) and report
  (`FP1-log-recovery-hunt-2026-09-05.md`) are in the recovery session's
  scratchpad — **they should be copied into `docs/plans/data/` before that
  scratchpad is reaped.** I have not done that: they are another session's
  files and I did not want to commit artifacts I had not produced or read in
  full.
- **Two long-standing facts were wrong and are corrected in the plan.**
  DroneOpsSync is an **Android APK on the controller**, not a Windows companion
  — a Windows companion was formally rejected in its ADR-0007, and NEXTL3VEL
  was never in the ingest path, which largely dissolves it as a lead. The seed
  of that misconception looks like a comment in
  `backend/tests/test_flight_ingest_consolidated.py`, **fixed on this branch**.
  Separately, the Synology Active Backup range is 10 versions,
  2026-05-29 → 2026-06-07 (not → 2026-09-01), with missed backups logged daily
  since 2026-06-08, and its repo is not shell-enumerable — the portal is the
  only path.
- **New P7 lead (not a dependency):** `2026-03-06_20-38-33_Open_Dronelog.db.backup`
  (95.6 MB, md5 `56a156aa…`) in two Synology Downloads archives, reporting
  **576** flights — a week newer than the DuckDB the plan cites. It may carry
  sha256s for some of the 36 files that currently match by filename only, which
  would upgrade them to hash matches in P7's verification step.

**Waiting on Bill:**
1. ~~Merge call on `feat/fp1-flight-details`~~ — **DONE 2026-09-05 02:35 PDT**,
   merged and deployed (verified below).
2. A `dji_m4t_official.png` asset for the new Matrice 4T fleet tile.
3. Nothing further on the 28 missing logs — they are gone, and the reason is
   now evidenced rather than assumed.

**Reminder:** a one-shot cron on HSH-HQ (`~/.local/bin/droneops-fp1-reminder.sh`,
fires 2026-09-11 09:00 PT, self-removes) emails Bill@BarnardHQ.com via
msmtp/O365 with this summary. **Now stale** — it was written to chase the
merge call, which has happened. It is harmless (one mail, then self-removes)
but its text will read as though the merge is still pending.

## 2026-08-17 — Encrypted R2 backup (ADR-0041) — LIVE, IN PARALLEL RUN — cutover pending

**The new lane is deployed, running on a timer, and verified end-to-end**
(V1–V12, see ADR-0041 "Implementation outcome"). **The old lane is still
running and has not been touched.** Both write every day. That is deliberate:
the plan requires **three consecutive green days** on the new lane before the
old one is removed. Removing it early is the one way to turn a working backup
into no backup at all.

**Green-day window opened:** 2026-08-17 (first timer-driven run 15:23 UTC).

**Soak tally** (timer-driven runs, `Result=success` + metric advanced, each
verified live over ssh — not inferred):
- 2026-08-17 15:23 UTC ✓ (completed 15:28:22)
- 2026-08-18 03:23 UTC ✓ (completed ~03:28)
- 2026-08-18 15:23 UTC ✓ (completed ~15:29)
- remaining: the 2026-08-19 pair + 2026-08-20 03:23 → then §5.7 fires
  **automatically**.

**Cutover is AUTOMATED (operator-approved 2026-08-18).** A user-level systemd
timer on droneops-server (`~/.config/systemd/user/droneops-backup-cutover.timer`)
fires `scripts/droneops-backup-cutover.sh` once at **2026-08-20 04:12 UTC**.
The script re-verifies every gate below over ssh BEFORE mutating anything
(≥6 completions, metric <13 h, `Result=success`, ≥4 restic db snapshots),
executes §5.7 (cron line out, plaintext R2 prefix deleted, `snapshot.sh`
retired), flips this doc, commits/pushes, syncs the BOS clone, and reports the
outcome — success or abort — to ntfy `infrawatch-alerts` with a click URL.
Any gate failure aborts before mutation. Dry-run tested 2026-08-18 under the
systemd user environment (correct refusal on the not-yet-met snapshot gate).
Cancel with: `systemctl --user disable --now droneops-backup-cutover.timer`
(on droneops-server). Note: criterion 4 (Sunday `--read-data-subset=5%`) is
satisfied by the manual deep-read checks in V2 + the DR rehearsal (both clean);
the first in-script Sunday run lands 2026-08-23, after cutover — accepted.

Also archived into this repo's restic repository during the window: the final
n8n database snapshot, tag `legacy-n8n` (see CHANGELOG 2026-08-17 entry and the
runbook lane table).

### Cutover criteria — ALL must hold before running §5.7

1. Three consecutive days with `droneops_backup_last_success_timestamp_seconds`
   advancing after **timer-driven** runs (not hand runs). Check:
   ```bash
   ssh 10.99.0.4 'systemctl list-timers droneops-backup.timer --no-pager;
                  journalctl -u droneops-backup.service --since "3 days ago" | grep -c "done\."'
   ```
   Expect ≥ 6 completions (twice daily × 3 days).
2. `obs-rule-droneops-backup-stale` has not fired in that window.
3. `restic check` green on every run (it is fatal in-script, so any failure
   would already have paged).
4. A Sunday `--read-data-subset=5%` run has completed at least once.

### Cutover commands (run on BOS-HQ `10.99.0.4`, in this order)

```bash
# 1. Retire the legacy cron line (leaves CallSign's line intact)
crontab -l | grep -v 'droneops/scripts/snapshot.sh' | crontab -
crontab -l                                   # verify only the callsign line remains

# 2. Retire the superseded script (history preserves it)
cd ~/droneops && git rm scripts/snapshot.sh && git commit -m 'ops(backups): retire snapshot.sh, superseded by droneops-backup.sh (ADR-0041) [skip-deploy]'

# 3. Delete the old PLAINTEXT R2 prefixes (~2.3 GiB, 229 objects)
#    ⚠️ Confirm the new repo has >=3 days of db snapshots FIRST.
docker run --rm --network host \
  -e AWS_ACCESS_KEY_ID=... -e AWS_SECRET_ACCESS_KEY=... -e AWS_EC2_METADATA_DISABLED=true \
  amazon/aws-cli s3 rm --recursive \
  --endpoint-url https://<R2_ACCOUNT_ID>.r2.cloudflarestorage.com \
  s3://obs-glitchtip-backups/droneops/
```

Then confirm the freshness metric keeps advancing for **three more days**
before declaring done.

### 2026-08-17 later the same day — cold DR rehearsal PASSED, four defects fixed

The lane was re-reviewed adversarially and rehearsed **cold**: rebuilt from the
1Password Fleet items and the R2 bucket only, on `droneops-server`, reading
nothing from BOS-HQ but comparison hashes and touching no production container
or volume. Restic-from-R2, the break-glass `.sql.gz` and live prod agreed on
every content digest; all 226 files in the `files` lane are sha256-identical to
production; the restored `.env` matches live byte-for-byte and the full stack
renders from it. **Recovery works cold — the filed secrets are sufficient.**
Evidence: `docs/runbooks/droneops-backup-restore.md` §11.

Fixed in `c3d9502`: missing `flock` concurrency guard; `backups/` not
gitignored (1.1 GB of plaintext PII dumps in the deploy clone's working tree);
the quarterly drill never reading the `files` lane; a post-metric error hole.
Plus a runbook defect — Procedure A2's first database command
(`docker compose up -d droneops-standby-db`) fails with `no such service`; the
service is `db-standby`.

Retention was challenged and **upheld**: a synthetic 40-day twice-daily corpus
converged exactly as designed. This does **not** change the three-green-days
cutover gate below — it remains the criterion.

### Also at cutover (do not forget)

- **Update the two Grafana rule descriptions.** `obs-rule-droneops-backup-stale`
  still instructs the operator to `tail ~/droneops/backups/snapshot.log` and
  re-run `snapshot.sh`. Replace with
  `journalctl -u droneops-backup.service -n 50` and
  `sudo systemctl start droneops-backup.service`. In
  `/opt/infrawatch/grafana/provisioning/alerting/observability-alerts.yml`.
  **Change the `description` text only — the metric names and expressions are a
  hard contract and must not move.**
- **Keep the local `.sql.gz` lane.** It is not part of the old lane being
  retired; it is the documented break-glass path that needs no
  `RESTIC_PASSWORD`.

### Open, for Bill

- **ntfy topic** stayed `infrawatch-alerts` per the 2026-07-14 "no new topics"
  decision, rather than the new `droneops-backup` topic the brief proposed.
- **7-year yearly retention** shipped per ADR-0041 D4, driven by the executed
  TOS PDFs and invoice records. Costs essentially nothing either way; confirm
  it matches the intended legal posture.
- **`~/backups/n8n_*.sqlite` on droneops-server** (~840 MB, root-owned, stops
  2026-04-15) — untouched, out of scope, needs a separate decision.
- **Legacy volumes** `droneops_postgres_data` (46 MB) and the `droneops-demo`
  stack — keep indefinitely, or schedule removal?

## 2026-07-06 — Download-link payment gate + automated delivery — SHIPPED (v2.79.0–v2.80.1, ADR-0039/0040)

**Complete and live on BOS-HQ.** Policy: clients never receive the
mission-footage download link until the invoice is paid in full; once it is,
delivery is fully automated (follow-up email + client-portal unlock) with no
operator steps. Both halves shipped, tested (delivery/gate suites incl.
endpoint-level e2e tests), and deployed. Verification pass on 2026-07-06
caught and fixed two bugs before they could bite: the SMTP-unconfigured path
stamping a delivery that never went out, and the `Mission.invoice`
lazy="noload" identity-map trap silently disabling the webhook/mission-update
triggers. Nothing in-flight; the only operator residual is outside the app
(revoking the pre-gate UI Drop share sent to River M. on 2026-07-02, and
re-minting it once paid — after which the automation takes over).

## 2026-06-24 — Async device-upload cross-container temp-handoff fix — SHIPPED + OPERATOR-CONFIRMED (v2.72.1)

**Deployed live to BOS-HQ 22:44 PDT 2026-06-24** (droneops.barnardhq.com →
2.72.1, healthy). **Operator-confirmed end-to-end the same evening:** a real
DJI Mavic 4 Pro flight log uploaded from the controller and **imported
successfully** — closing the ADR-0023 §5 operator gate. Because the log
imported (not merely uploaded), the DJI v13+ AES decryption path
(`dji_api_key` → `X-DJI-Api-Key` on `flight-parser`) is also confirmed working;
that dependency had never been exercised before because the file never reached
the parser. **ADR-0023 is now fully satisfied.**

The shipped ADR-0023 async path failed for every real upload: the API
container handed the worker a `/tmp` spool **path**, but the worker runs in a
**separate container** and can't see that `/tmp` (`[Errno 2] … '/tmp/flight_upload_*'`).
Field-reported on a DJI Mavic 4 Pro log (2026-06-24). Fixed by having the
worker read the original from the **shared `app_data` hash store**
(`_get_stored_file_path`), plus closing the now-redundant `/tmp` spool in the
route (it was leaking on the backend). Two regression tests added that cross
the container boundary the old harness mocked away. Full suite green
(433 passed, 3 skipped). Detail: CHANGELOG 2026-06-24 + **ADR-0023 §6**.

**Closed:**
- ✅ Live on BOS-HQ + operator-confirmed M4P upload imported (see header).
- ✅ DJI v13+ AES decryption path (`dji_api_key` → `X-DJI-Api-Key`) confirmed
  working — the import succeeded, so the parser decrypted the log.

**Closed follow-up:**
- ✅ **v2.72.2** — hardened: the async route verifies the original is resolvable
  on the shared store before enqueueing; if not, the file is `error` in the 202
  body and no job is enqueued (no 202 for a file the worker can't read).
  `_spool_upload`'s fail-soft contract unchanged (legacy sync route unaffected).
  See CHANGELOG 2026-06-24 + ADR-0023 §6.

_No open follow-ups remain for the async device-upload work._

## 2026-06-15 — Device-upload async decoupling (audit P2-2) — DESIGNED (not started)

The last open item from the 2026-06-11 ground-up audit (P2-2 full leg) is now
designed. Analysis + docs only; no application code touched.

- **Decision (ADR-0023):** add a **separate** async route
  `POST /api/flight-library/device-upload/async` (202 + `{batch_id}`) + a poll
  route `GET .../device-upload/status/{batch_id}`, mirroring the v2.70.0
  backup-job pattern (`backup_jobs.py` + `run_backup_job_task` +
  `/api/backup/jobs`). The legacy synchronous `device-upload` route stays
  unchanged forever (native APK = no OTA, so old field devices must keep
  working). Separate route chosen over a capability header because the
  response shape/status code differ — two clean contracts beat one URL with
  two behaviours.
- **batch_id granularity:** keep one-file-per-request (one `batch_id` per
  file); the contract also supports multi-file submit but the first client
  release won't use it (per-file = better field reliability + 1:1 onto the
  existing `UploadStatus`).
- **Client (DroneOpsSync ADR-0008):** the **socket-timeout-is-per-file** fix
  (`MainViewModel.kt:721-725` — drop `aborted = true`) is a one-line,
  backend-independent reliability win recommended as a **standalone fast-follow
  APK** ahead of the async leg. `UnknownHostException` + 401/403 keep
  `aborted = true` (correctly batch-wide). Timeout retune deferred to the
  async-adopting release (lowering it before the parse moves off-request would
  amplify the hang).
- **Plan:** `docs/plans/2026-06-15-device-upload-async-decoupling.md` —
  Stage A (redis module) → B (route+task+poll, ships backend-first) → C (client
  fast-follow, parallel) → D (client async adopt, after B) → E (docs).
- **Verification of audit claims:** all file:line claims confirmed against
  current source. One nuance: `performUpload` also sets `aborted=true` on HTTP
  401/403 (`:690`) — that one is correct and is left in place.
- **No data-loss risk today:** SHA-256 server-side dedup already makes every
  path idempotent; the brittleness costs bandwidth + a confusing half-failed
  sync UX, not lost flights.
- **Owner:** aegis (backend leg) + fleet-mobile-engineer/aegis (client leg).
  Not started.

## 2026-06-02 — Flight date timezone fix + deploy-path correction — SHIPPED (live)

**Flight date bug (v2.68.1, ADR-0017).** An evening flight flown 2026-06-01
20:27 PDT displayed and was named `..._20260602_...`. The instant was captured
correctly (stored naive-UTC `2026-06-02 03:27` *is* `2026-06-01 20:27 PDT`); the
bug was reducing that UTC instant to a date with no operator-timezone
conversion, in two places. Fix: `backend/app/utils/timezone.py` (single
UTC↔operator-local source, `OPERATOR_TIMEZONE` default `America/Los_Angeles`);
all flight datetimes serialize UTC-aware (`iso_utc`, `+00:00`); frontend
`src/lib/datetime.ts` formats pinned to the operator TZ (viewer-independent);
`_generate_flight_name` uses local date. Backfill `scripts/backfill_flight_local_dates.py`
re-stamped 29 existing names (idempotent, auto-resequences collisions).
Display self-corrects retroactively; PDFs unaffected (use `mission_date`).
Built + deployed to BOS-HQ, verified live (the flight now reads "Jun 1, 2026,
8:27 PM PDT"). 6/6 new tests pass.

**Deploy path corrected (ADR-0018 + NOC-Master ADR-0079).** Discovered the repo
was in a half-migrated state: the per-repo `update.sh` was deleted in `e4610b5`
(migrate to the NOC fleet deployer) but `autopull.sh` + the systemd units were
left behind referencing it — dead, misleading. Retired them (ADR-0018). The
*actual* reason pushes weren't going live: the NOC deployer's image-digest gate
was structurally blind to this repo's `build:`-only compose services (no
`image:` name), so it reported "success" while rebuilding nothing — a silent
stale deploy. Root-caused + fixed deployer-side in **NOC-Master ADR-0079**
(`extractImageRefFromCompose` now resolves the compose-default
`<project>-<service>:latest`). Verified: a push now rebuilds + recreates on
BOS-HQ automatically. **Deploy path of record = the NOC fleet deployer
(`swarmpilot_deployer` on HSH-HQ); watch at
https://noc-mastercontrol.barnardhq.com/deploys. There is no per-repo autopull
anymore — do not recreate it.**

## 2026-05-25 — AI report-gen Celery async-loop bug fixed + Opus 4.7 — SHIPPED

Report generation was flaky/failing: the Celery tasks doing async DB work reused
the module-global `async_session` across per-task event loops → asyncpg
`got Future attached to a different loop` / `Event loop is closed`. `generate_report`
only recovered on retry (hard-fail after 3); `send_payment_reminders` (dunning) had
no retry and silently failed. Fixed via `app/tasks/async_db.py` (fresh loop +
task-local NullPool engine per task); both tasks migrated; `send_report_email`
unaffected (no DB). Provider/key/model were already correct (claude + key set);
made `claude_model` a per-instance DB setting and set this instance to
`claude-opus-4-7`. 3 new regression tests; suite 279 passed.

## 2026-05-24 — business-signals tz bug fixed — SHIPPED (06365ef)

`GET /api/v1/business-signals` compared tz-aware window bounds
(`datetime.now(timezone.utc)`) against tz-naive `paid_at`/`updated_at`/
`created_at` columns; asyncpg raised `DataError`, `_safe_scalar` swallowed it,
and every windowed metric silently returned 0/null. Project J.A.R.V.I.S. (the
consumer) had been getting zeroed innovation signals. Fixed via a new
`_utc_windows()` helper returning tz-naive UTC bounds (+ `generated_at` now
carries `Z`); 3 regression tests pin the invariant. Read-only query fix — no
schema/writes/failover impact. Verified live on BOS-HQ: `invoice_paid_usd` 30d
`0 → 1216.36`, `missions_completed` `null → 1`. Also unblocked the marketing
revenue bridge's 30/90-day windows (it had routed around this via
`financials/summary`).

## 2026-05-24 — Invoicing hardening + dunning + portal/nginx fixes — SHIPPED (v2.67.7)

All shipped to `main` and deployed to the public instance
(droneops.barnardhq.com); images rebuilt 2026-05-24.

- **Invoice engine:** recompute-at-charge + atomic save (stale-total fix),
  live exact-50% deposit, decimal hours + "Hours" label, tax-rate 100x fix,
  legacy-wizard atomic save; mobile invoice-editor + Mission Hub card UX.
- **Dunning (payment reminders):** 48h gentle reminder + 7d final notice +
  operator-overdue email (email-only, no ntfy). Daily Celery-beat sweep at
  16:00 UTC, themed emails, sign-off "Bill Barnard — BarnardHQ". Banks invoice
  BARNARDHQ-2026-0002 enrolled; BOS cron confirms the 48h fire 2026-05-26
  16:35 UTC.
- **Client-portal links:** broken plural `/client/missions/` route fixed in
  the Stripe success/cancel redirect + dunning fallback (→ `/client/mission/`);
  `?payment=cancelled`→`cancel`.
- **nginx resilience:** `frontend/nginx.conf` uses `resolver` + variable
  `proxy_pass` so a backend rebuild no longer 502s `/api` (verified by forcing
  a real backend IP change). Closed a live 502 that surfaced as a false
  "link expired".

### Deployment state (all current as of 2026-05-24)
- **Public** (`~/droneops`, https://droneops.barnardhq.com) — ✅ v2.67.7.
- **Demo** (`~/droneops-demo`, https://command-demo.barnardhq.com) — ✅ updated
  to v2.67.7 on 2026-05-24 (backend/frontend/flight-parser rebuilt; DB
  self-migrated via the `main.py` `_add_missing_columns` startup helper).
  **worker/beat intentionally left stopped** so the demo never sends real
  dunning email — re-confirm they stay down on any future demo `up`.
- **Managed** (`~/droneops-managed`) has **zero active clients**; the managed
  template builds from `droneops-backend:latest`/`droneops-frontend:latest`
  (rebuilt today), so future clients auto-inherit these fixes.

### Open / follow-ups
- **SECURITY (operator action):** scrubbed plaintext GH tokens from
  `~/droneops/.git/config` and `~/droneops-demo/.git/config` on BOS (now bare
  URLs + credential helper). The exposed `ghp_…` token should be **rotated at
  GitHub** — overwrite the single line in `~/.secrets/git-credentials` after.
- **Deferred:** SMS reminders (Phase 2); deployer self-roll fix (queued in
  noc-master).

## 2026-05-14 — Mission Report: clear stale draft on Generate click — CLOSED

Single-file UX tweak on top of the ADR-0015 work. `handleGenerate` in
`frontend/src/pages/MissionReportEdit.tsx` now clears `reportContent`,
`hasAudienceLeak`, and `audienceLeakDetails` immediately before firing
the POST so the operator gets instant visual confirmation. PDF + Send
naturally disable during the in-flight window via their existing
`!reportContent` guards (no new disabled-state logic introduced).
Re-entrant Generate clicks were already a no-op via the existing
`disabled={generating || !narrative}`.

Verification:

- 8/8 tests in `frontend/src/pages/__tests__/MissionReportEdit.test.tsx`
  pass (new test `Generate Report clears the existing draft content
  immediately` locks the behavior; CONTRACT test reordered so Generate
  runs LAST — load-bearing claim unaffected).
- `tsc --noEmit` clean on the frontend.

Out of scope this round (operator flagged for future iteration):
broader **report quality** is "ok for now but needs to get better."
That's prompt-quality + detector-coverage work for a later pass.

## 2026-05-14 — Mission-report audience leak — CLOSED on docs + RCA + prompt fix; runtime gate IN-FLIGHT

Quality defect on the LLM-generated mission report. Operator-only catch,
no customer impact. Planned-work close-out workflow.

**Status:**

- **RCA + prompt fix (aegis) — COMPLETE at commit `22469ed`.**
  - System prompt rewritten in `backend/app/services/ollama.py:9-38`
    (names CLIENT as reader, names operator as upstream author, forbids
    second-person address + four common leak phrases, reframes Section
    5 to "Client Follow-Up Items" with OMIT-fallback).
  - User-prompt block in both providers updated
    (`ollama.py:75-89` + `claude_llm.py:52-66`) — operator notes
    labeled `CONTEXT ONLY` with translation instruction; trailing
    "Generate the client-facing after-action report" with audience
    constraint repeated. Belt-and-suspenders against drift on long
    contexts.
  - New regression suite: `backend/tests/services/test_report_audience_guard.py`,
    17/17 passing on Python 3.12.3. Layer 1 (4 tests) locks prompt
    structure; Layer 2 (13 tests) exercises the deterministic detector
    against representative bad phrasings + the verbatim shape of the
    operator-reported leak. Hermetic, ~1.8s. Pre-existing 2 failures
    in `test_health_stripe_db_lookup.py` reproduce on pristine `main`
    HEAD; out of scope.
  - New detector module: `backend/app/services/report_audience.py`
    (`detect_audience_leaks`, `has_audience_leak`, `AudienceLeak`
    dataclass — nine rule categories). Exposed as stable callable to
    enable the runtime gate without re-implementing the rules.
  - `SYSTEM_PROMPT_TEMPLATE` deliberately left in `ollama.py` for this
    commit; aegis's CHANGELOG entry calls out that the relocation to
    `llm_prompts.py` is left for a follow-up to keep the surgical
    surface tight. FU-AI-3 status on ROADMAP updated accordingly.

- **Documentation close-out (Terry) — COMPLETE.**
  - `docs/adr/0015-mission-report-audience-separation.md` — flipped
    Proposed → Accepted with the stronger scope ("operator-facing
    coaching is explicitly out of scope"); added §"Rejected
    alternative: operator-facing debrief surface" capturing the
    operator's verbatim rejection; added decision #5 for the runtime
    soft-block gate.
  - `docs/incidents/2026-05-14-mission-report-audience-leak.md` — §9
    open questions reconciled against aegis's findings (Q1 closed at
    `22469ed`, Q2/Q3 operator-action open, Q4 deferred low-priority,
    Q5 24h-soak cadence decided); new §10 "Decisions made post-RCA"
    captures both operator decisions verbatim.
  - `ROADMAP.md` — FU-AI-1 (operator retrospective) removed entirely;
    FU-AI-2 marked SHIPPED at `22469ed`; FU-AI-3 marked
    DE-PRIORITIZED with standalone rationale; FU-AI-4 retained with
    explicit standalone justification (tenant tone-override is not
    tied to the dropped surface); new FU-AI-RUNTIME-GATE item added.
  - `CHANGELOG.md` — narrative `[unreleased]` entry recording the two
    operator decisions, sequenced above aegis's code entry to reflect
    that the decisions resolve the "Out of scope (flagged for operator
    decision)" items aegis flagged.

- **Runtime soft-block gate wire-in (aegis) — COMPLETE at commit
  `4953edf` (2026-05-14, local; deploy pending operator review).**
  - Detector wired into the **persistence site** of report generation
    rather than the per-provider call paths. Both providers
    (`claude_llm.py`, `ollama.py`) funnel through `llm_provider.generate_report`
    which returns a plain string; `generate_report_task` in
    `backend/app/tasks/celery_tasks.py` is the sole place that string
    becomes a row — one wire-in covers both providers.
  - New helper `_apply_audience_findings(report, llm_content)` runs the
    detector after `final_content` is set on the row and persists results
    into two new `Report` columns:
    - `has_audience_leak BOOLEAN NOT NULL DEFAULT FALSE`
    - `audience_leak_details JSONB NOT NULL DEFAULT '[]'::jsonb`
  - Migration via the existing idempotent `_add_missing_columns` path
    in `backend/app/main.py:114-122` (repo convention; no Alembic).
    Failover-safe per CLAUDE.md §Failover Guard.
  - Helper **never raises** — detector failure logs + leaves flags at
    defaults so generation never 500s. **No regen loop** per operator
    directive; doc-string-lock test prevents drift toward retry-clean.
  - `ReportResponse` schema + frontend `Report` type carry the new
    fields; `MissionReportEdit.tsx` renders a yellow `IconAlertTriangle`
    Mantine `Alert` banner above the FINAL REPORT editor when the flag
    is true, listing each matched phrase with its rule name. Save / PDF
    / Send remain enabled — editorial review IS the gate.
  - Test coverage: 10 new hermetic tests in
    `backend/tests/services/test_audience_leak_persistence.py`
    (helper behavior + Pydantic round-trip + soft-block doc lock).
    Existing 17-test audience suite stays green. Full backend suite:
    240 passed, 1 skipped, 2 failed (the pre-existing
    `test_health_stripe_db_lookup.py` failures, unchanged).
  - Operator-debrief surface DROPPED per operator decision (Terry's
    ROADMAP edit captures the rejection). The detector + banner pair
    is the chosen final surface.

- **Verification gate.** Personal-instance soak 24h with a real report
  generated against the new prompt before any push to managed-hosting
  tenants. No same-day fan-out. Operator decision per the planned-work
  close-out preference.

**Coordination note for aegis on shared touch surface:** aegis already
landed a CHANGELOG entry for `22469ed` (the code patch); Terry's
narrative entry on the operator decisions is intentionally
**separate** rather than amended to aegis's, so the *decisions* (drop
debrief surface, approve soft-block) read cleanly in the ledger as
their own narrative beat. When aegis's runtime-gate code commits, the
CHANGELOG entry for that commit should be a third `[unreleased]`
block referenced from ADR-0015 §Decision-5 — not folded into either
of the existing two.

## 2026-05-14 — Mission-report overall quality — OPEN BACKLOG (watching brief)

Operator feedback at the ADR-0015 close-out, verbatim: *"its ok for
now but it needs to get better."* Referring to the overall
client-facing mission report quality, not any specific defect. No
specific changes requested; this is a signal that the current quality
bar is not the destination.

**Status.** NOT STARTED. Tracked on ROADMAP as `FU-AI-QUALITY-PASS`
under the "LLM-assisted report surface" section. The audience-separation
contract from ADR-0015 is load-bearing; any quality work happens
inside that contract.

**Operating rule for future sessions.** Do not assume a direction and
start editing the prompt or the report template. When asked to "improve
the report," first ask the operator what specifically he wants
improved. Candidate areas captured on the ROADMAP entry are inference
for kickoff, not a committed punch-list. Trigger to act is
operator-driven.

## 2026-05-03 — v2.66.0 backend hardening (Agent A — IN-FLIGHT, awaiting orchestrator merge)

Branch: `feat/v266-backend-hardening`. All 8 P0/P1 fixes implemented;
27 new tests pass; full suite 143/143 green. ADR-0011 written; CHANGELOG
entry added; version bumped in all 4 files (README, package.json,
backend/main.py, AppShell.tsx).

Cuts decided:

- **Cut 1 (duplicate operator routes):** DONE. ~165 lines of dead code
  removed from `routers/client_portal.py`. Route registry now shows
  exactly 3 client-link routes.
- **Cut 2 (managed_instance auto-provision):** KEEP. Used in 5 places.
- **Cut 3 (Ollama):** KEEP. `droneops-ollama-1` healthy on BOS-HQ.
- **Cut 4 (Demo middleware):** KEEP. `droneops-demo-*` running.
- **Cut 5 (/api/branding):** KEEP. `useBranding.ts` consumes it.

Coordination notes for orchestrator:

- Agent B will also touch `frontend/src/components/Layout/AppShell.tsx`
  (frontend P0/P1). I bumped the v2.65.1 → v2.66.0 footer string in
  both occurrences. Merge mine first; Agent B rebases.

## 2026-04-24 LATE — COMPLETE: Performance audit fix series (ADR-0004/0005)

Executed the 5-fix plan from `docs/plans/2026-04-24-perf-audit.md`.
All 5 fixes shipped as separate commits, each with version bump,
auto-merged into main, deployed to BOS-HQ via NOC autopull, and
verified live with the §6 acceptance commands. ADR-0004 + ADR-0005
both `accepted`.

- **FIX-1 — v2.63.7:** weather `asyncio.gather` + 5-min Redis cache
  (failure-open). Cold 7.4-8.3 s → **1.09 s** (6.8-7.6×); warm 7.4 s
  → **6-19 ms** (~390-1200×). 6 cache tests green.
- **FIX-2 — v2.63.8:** pool 5+10 → 20+20; 60 s in-process cache around
  `get_current_user`; explicit invalidate on password/username change.
  30-parallel `/api/customers` cold p95 0.67 s, warm p95 **0.27 s**.
  5 user-cache tests green.
- **FIX-3 — v2.63.9:** 17 main pages → `React.lazy` + Vite
  `manualChunks` for vendor bundles. Main `index-*.js` 1.9 MB →
  **81.4 KB** (23.5×).
- **FIX-4 — v2.63.10:** `useApiCache` hook + Dashboard + Flights/aircraft
  adoption with mutation invalidation. Bundle unchanged (no regression).
  Settings deliberately scoped out (large mutation surface; explicit
  scope decision, not a stopgap — see ADR-0005 §FIX-4).
- **FIX-5 — v2.63.11:** ADR-0004 + ADR-0005 finalized; CHANGELOG +
  PROGRESS finalized. Audit series complete.

All three §6 acceptance thresholds passed. Failover guard never
violated. No new dependencies. No deferred fixes (only scope decisions
with explicit rationale).

## 2026-04-24 EVENING — SHIPPED PR: Zero-touch device API key rotation (ADR-0003 / FU-7)

Backend v2.63.6, paired with DroneOpsSync v1.3.25. Closes the manual key-paste step that the 2026-04-24 morning incident required.

**Branch:** `claude/zero-touch-key-rotation-backend` (PR open, **not merged** — operator reviews per the routine spec).

**What landed:**

- Two nullable columns on `device_api_keys` (`rotated_to_key_hash`, `rotation_grace_until`); additive, failover-safe per CLAUDE.md §Failover Guard.
- Dual-key auth in `validate_device_api_key` during grace.
- New admin endpoint `POST /api/admin/devices/{device_id}/rotate-key`.
- Redis side-channel for the raw new-key hint (`app.services.rotation_hint`); fail-closed if Redis is down.
- Device-health response emits `rotated_key` + `rotation_grace_until` ONLY for OLD-key auth during grace; transparent to existing clients.
- Celery finalizer task on 15-min beat (`finalize_key_rotations_task`).
- Single Pushover FYI per rotation; env-gated like the rest of ADR-0002 §5.
- 15 unit tests, all green; `backend/tests/` infrastructure bootstrapped (`pytest.ini`, `conftest.py`, `requirements-dev.txt`).

**Notable about the routine handoff:** Claude Code remote routine `trig_01KiBK88vqs6vtRf75rkxcw8` shipped an empty branch on its first run. aegis re-ran the spec from this conversation and produced both PRs. ROADMAP FU-7 closed.

**Not deployed yet** — operator merge → `update.sh` rebuild on BOS-HQ → Celery beat picks up the new task on next worker restart.

## 2026-04-24 — SHIPPED: DroneOpsSync prevention mechanisms + landscape lock (ADR-0002 §5)

Backend v2.63.5 / companion v2.62.1. Bill's uploads are recoverable per §4.1 (operator paste the rotated `M4TD` key on his RC Pro); this follow-up makes the class of failure non-recurrent.

**Landscape lock** — `patch-android.cjs` injects `sensorLandscape` + `configChanges` on every `<activity>` after `npx cap sync android`, with a build-time fail-hard if any `portrait` survives. DJI RC Pro is physically landscape-only; a rotate reflow would destroy the WebView.

**Layered silent-drift watchdog (all on by default):**
1. Companion pairing banner on launch via `checkPairing()` — persistent red banner when `serverUrl` or `apiKey` is missing/malformed. Blocks auto-sync that could only fail.
2. Companion preflight health gate via `preflightHealth()` — structured `{ok, code, message}`; failures surface as banner copy, not silent retries.
3. Server silence watchdog — hourly Celery beat (`check_device_silence_task`). Recently-active keys silent > 48h fire a Pushover alert, deduped 12h. New `beat` compose service.
4. First-401 Pushover alert — `validate_device_api_key` on any `/device-*` path, deduped 1h per `(key_prefix, ip)`.

Push alerting is env-gated: `NTFY_DRONEOPS_PUBLISHER_TOKEN`. Unset = structured JSON log only (still observable via Loki). No flag-gating anywhere. **Migrated from Pushover to ntfy on 2026-04-26 per ADR-0036 + ADR-0006 addendum** — same dedup, same Redis suppression, same `send_alert` signature; only the transport changed (now `https://ntfy.barnardhq.com/droneops-watchdog` with publisher-side fallback to `ntfy.sh`).

**Open action for operator:**
- Drop `NTFY_DRONEOPS_PUBLISHER_TOKEN` into BOS-HQ `~/droneops/.env` to turn on phone alerts (already populated by ADR-0036 Wave 2 bootstrap). Without it, the watchdog still runs (visible in `droneops-beat` logs + Loki) but Bill's phone stays quiet.
- Next APK install will apply the landscape lock + banner. Pending: GitHub Actions on `main` will publish `DroneOpsSync-2.62.1.apk` via the self-hosted BOS-HQ runner (ADR-0029).

## 2026-04-24 — Awaiting operator action on Bill's 3 pending flight records (ADR-0002 §4.1)

Status: server healthy, `M4TD` key rotated + verified (HTTP 200 end-to-end from HSH-HQ to BOS-HQ via CF). The stale-APK RCA in the v2.63.4 commit was wrong; the actual root cause is Capacitor `Preferences` state on Bill's RC Pro. Second-pass evidence in `docs/adr/0002-droneopssync-upload-auth.md` §4.1.

Pending: Bill paste `doc_m4td_i8Qt9OJDogxjbgXgz2LRH4a0MrzTSxcVa8ltHxoS0Us` into DroneOpsSync → Settings on his RC Pro, tap Test Connection (green = M4TD), tap Sync Now. The three `DJIFlightRecord_2026-04-23_*.txt` files upload. Follow-up telemetry: `M4TD.last_used_at` should advance past `2026-04-19 23:07:44` and three `device_upload` INFO log lines should appear in `droneops-backend-1`.

Follow-up (not blocking today's records): v2.62.0 APK install to pre-bake `DEFAULT_SERVER_URL = https://droneops.barnardhq.com` so future Preferences wipes can't silently break uploads on any device in the fleet.

## 2026-04-24 — DroneOpsSync upload auth + HTTPS-only base URL (ADR-0002)

Operator's personal DJI RC Pro (no camera) could not upload 3 post-flight
logs (~17 MB) to `http://droneops.barnardhq.com`. Two-symptom failure:

1. `/health` GET returned HTML (CF HTTP→HTTPS redirect body); stale APK's
   Gson client crashed at `line 1 column 1` because it ran with default
   `setLenient(false)`.
2. Upload POST returned `403 {"detail":"Not authenticated"}` — FastAPI's
   default `get_current_user` JWT rejection, i.e. stale APK hit a
   JWT-gated endpoint instead of the current `X-Device-Api-Key`-gated
   `POST /api/flight-library/device-upload`.

Root cause: the APK on the controller is pre-v2.33.0, pre-dates the
Capacitor rewrite, and uses a Gson-based client against legacy paths.
The current server surface is correct — device-health and device-upload
endpoints are already wired to `validate_device_api_key`
(`backend/app/auth/device.py`) with SHA-256 hash lookup.

**Status — SHIPPED 2026-04-24 by aegis.** Backend v2.63.4, companion
v2.62.0. Scope delivered:
- Companion: `validateServerUrl()` in `sync.ts` rejects plaintext public
  URLs with RFC-1918 + loopback carve-out; `DEFAULT_SERVER_URL`
  pre-baked to `https://droneops.barnardhq.com`;
  `App.tsx::saveAndSync` catches validation errors into the Settings
  test-status banner. Footer bumped. APK will be cut by
  `companion-apk.yml` on BOS-HQ self-hosted runner on push.
- Server: top-level `GET /health` alias (JSON, same payload as
  `/api/health`); structured INFO log on `/device-upload` and WARN log
  on device-auth failure (`key_prefix` only — never the raw key).
- Operator: existing `M4TD` row in `device_api_keys` is already valid
  (last used 2026-04-19); no rotation. Bill reuses the raw key value
  he already has.

**Pending operator action**: install `DroneOpsSync-2.62.0.apk` from the
upcoming release on the RC Pro; paste server URL + existing `M4TD` key;
tap SAVE & SYNC. The 3 pending DJIFlightRecord files upload.

Docs:
- **ADR-0002** (`docs/adr/0002-droneopssync-upload-auth.md`) — auth-model
  decision + HTTPS-only + forward path for managed-tenant discovery
  (deferred; pattern copy from EyesOn ADR-0020 when first tenant ships).
- **CHANGELOG** — 2026-04-24 entry above ADR-0029.
- **ROADMAP** — follow-up items (fleet audit, `/health` shim, Grafana
  stale-client tripwire) filed under "Observability + Fleet Hygiene".

Hardware constraint: DJI RC Pro has no usable rear camera for field
operation. No QR / visual pairing ever. Auth model aligns with EyesOn
ADR-0019 (keypad / non-visual enrollment). See
`feedback_dji_rc_pro_no_camera.md`.

## 2026-04-20 — Maintenance type vocabulary unified (v2.63.3)

Fixes a long-standing bug where overdue schedule alerts (Compass
Calibration et al.) could not be cleared via "+ Log Maintenance".

- Frontend `MAINTENANCE_TYPES` now mirrors backend
  `DJI_MAINTENANCE_DEFAULTS` exactly — 10 DJI categories + `General
  Service` + `Other`, Title-Case as both value and label.
- Migration script `scripts/migrate_maintenance_type_vocab.py` rewrites
  legacy snake_case record rows → canonical Title-Case. Idempotent.

**Status** — HSH-HQ prod: v2.63.3 live `2026-04-20`, 5 legacy records
remapped to Title-Case via the migration script. User logs a Compass
Calibration record per affected aircraft through the UI to clear the
three overdue schedules (now possible because the dropdown has the
option and the backend schedule-match will fire).

**Deferred — CHAD-HQ demo:** still on `dfad0a3`. `git pull` blocked by
uncommitted operational fixes on `docker-compose.demo.yml`,
`docker-compose.standby.yml`, `.env.demo` (port-binding hardening +
primary_conninfo IP correction). Demo has **zero maintenance records**
so the migration would be a no-op there anyway. The frontend fix for
demo can land when someone reconciles the uncommitted compose edits
— scope for a separate session.

## 2026-04-19 — Zombie-leak incident (RESOLVED)

Completed: zombie-leak fixes + Redis-heartbeat healthcheck.

- **v2.63.2** (commit `897c78a`) — Redis-heartbeat docker healthcheck.
  Replaces `celery inspect ping` subprocess (which re-imported the full OTel
  chain every 60s) with a lightweight Redis age check. Worker's
  `worker_heartbeat` signal writes unix-ts to `droneops:worker:heartbeat`
  (120s TTL); healthcheck is `redis-cli GET + age < 60s`. Interval 30s,
  timeout 5s, start_period 30s. Fast path, resilient to Redis brief outages.

- **Ops** (commit `98f7309`) — Backend zombie-leak fix (follow-up).
  Investigation found 3 fresh `<defunct>` curl children accumulating under
  uvicorn master. Same SIGCHLD reap leak pattern as worker, different
  container. Added `init: true` (tini PID 1) to backend service in compose.

- **Ops** (commit `9ae3c95`) — Worker zombie-leak fix (primary).
  HSH-HQ high-load incident found 33 defunct celery children accumulating
  ~2/hr over 18h. Root cause: celery prefork master loses occasional SIGCHLD
  reaps on Python 3.12. Added `init: true` (tini) + `--max-tasks-per-child=50`
  to worker service. Per-child task cap keeps leaked children short-lived;
  tini as PID 1 makes the leak structurally impossible.

### Repair quality
- All changes compose-only; no application code touched.
- Failover-safe: per-container health signals, no cross-container state.
- Docker inspect: confirms `redis-cli` present in new backend image.
- Roundtrip tested: SETEX/GET on redis:7 (stack image).
- Incident log: `~/noc-master/docs/incidents/2026-04-19-hsh-hq-high-load.md`.

## 2026-04-18 — Observability Phase 5 (COMPLETE on code side)

Ships in two functional commits + one doc commit:

- **v2.63.0** (commit `6b7e626`) — structured JSON logging pre-req.
  Replaces plain `logging.basicConfig` with `python-json-logger` on
  root + Celery `after_setup_logger` signals.
- **v2.63.1** (commit `d4df8e7`) — Sentry + OTel SDKs + compose labels.
  Backend `app/observability/` package, frontend `src/lib/sentry.ts`,
  `com.barnardhq.*` labels on every service, demo override pins CHAD-HQ
  Alloy + `env=demo`.
- **docs** (this commit) — ADR-0001, PROGRESS.md, CHANGELOG.md entries.

### Deploy + verification

1. NOC Master (swarmpilot) picks up the push on `main` and runs the
   prod build/up sweep on HSH-HQ.
2. Demo build on CHAD-HQ: `cd ~/droneops && ./bootstrap.sh` (or the
   documented `docker compose -p droneops-demo -f docker-compose.yml -f
   docker-compose.demo.yml --env-file .env.demo up -d --build`).
3. Verify on BOTH hosts per `reference_droneops_topology.md`:
   - Loki: `{service="droneops-api",env="prod",host="hsh-hq"}` +
     `{service="droneops-api",env="demo",host="chad-hq"}` return JSON
     lines.
   - GlitchTip: issue in `droneops` project tagged `env=prod` and
     `env=demo` (separate).
   - Tempo: `service.name=droneops-api` trace on an `/api/health` hit.
   - `docker exec <container> curl localhost:8000/api/health` returns
     200 on both hosts.

### Env vars required at deploy time

On each host's `.env` / `.env.demo`:

- `SENTRY_DSN` — from
  `/home/bbarnard065/.secrets/observability-dsns.env::DRONEOPS_API_SENTRY_DSN`.
- `VITE_SENTRY_DSN` — from that same file::`DRONEOPS_FRONTEND_SENTRY_DSN`.
- `OTEL_EXPORTER_OTLP_ENDPOINT=http://10.99.0.1:4317` on HSH-HQ,
  `http://10.99.0.2:4317` on CHAD-HQ (or leave the demo override's
  default).

Unset = no-op. Nothing in the app code fails if these are absent.

## Follow-ups

- **Companion APK instrumentation.** Per
  `feedback_droneops_companion_apk.md`, the Android companion at
  `~/droneops/companion/` (Kotlin) is not instrumented in Phase 5. If
  the companion needs `SentryAndroid.init`, that's a separate commit +
  an APK rebuild + release — flagged for the user, not scoped here.
- ~~**Managed-hosting branch.**~~ Resolved: `managed-hosting-v2` merged
  `2026-04-10` as v2.62.0 (merge commit `85f28e9`). `.env.example` already
  carries the observability block (`SENTRY_DSN`, `OTEL_EXPORTER_OTLP_ENDPOINT`,
  `VITE_SENTRY_DSN`, etc.), so managed instances get the hooks by default;
  operators just paste the DSN.
- **Dashboards (Aegis-F / Phase 7).** DroneOps-specific Grafana
  dashboards aren't in scope for this phase; Aegis-F is planning them.

---

## 2026-04-24 PM — Capacitor companion abandoned; Kotlin path restored; v1.3.25 queued

### Completed
- `companion/` directory deleted from this repo (commit `4b87e65`, 20 files / 5,306 lines). Abandoned Capacitor fork had zero device installs after 4 weeks of parallel-track work. ADR-0002 §7 added cross-linking DroneOpsSync ADR-0001. Workflow `.github/workflows/companion-apk.yml` removed.
- Three orphan GH releases (`companion-v2.61.5`, `companion-v2.62.0`, `companion-v2.62.1`) edited with `⚠ ABANDONED` banner pointing at DroneOpsSync repo.
- BOS-HQ Pushover watchdog verified end-to-end. `~/droneops/.env` already had `PUSHOVER_TOKEN` + `PUSHOVER_USER_KEY` set this morning (06:51Z); container `droneops-backend-1` has both vars loaded; `curl api.pushover.net/1/users/validate.json` returns `status:1, devices:["PrimaryPhoneS25"]`. Smoke-test notification `3eac9d51-fc67-4248-8a8a-038daac5a023` delivered to Bill's phone. ADR-0002 §5 layers 3+4 can now page the operator.

### Scheduled (not yet executed)
- Remote routine `trig_01KiBK88vqs6vtRf75rkxcw8` (https://claude.ai/code/routines/trig_01KiBK88vqs6vtRf75rkxcw8, fired 2026-04-24T18:58Z) will open PRs for zero-touch device API key rotation. This repo lands ADR-0003 + backend grace-window + dual-key auth + celery finalizer + Pushover FYI. Paired with DroneOpsSync v1.3.25 that parses the `rotated_key` field from preflight response.

### Decisions
- Real DroneOps companion app lives in `BigBill1418/DroneOpsSync` (native Kotlin), NOT in this repo. A future session that wants to "fix the companion" MUST work there. `feedback_verify_before_planning` applied — verifying which codebase actually runs on Bill's RC Pro before planning should have been step 1 this morning, not step N.
- Pushover token in `~/droneops/.env` is a dedicated DroneOps app (distinct from NOC's), giving DroneOps its own rate-limit bucket and notification identity on Bill's phone. USER_KEY matches NOC canonical.

### Evidence
- DroneOpsCommand `main` HEAD: `4b87e65` (companion removal).
- DroneOpsSync `main` HEAD: `832585c` (v1.3.24 released, 4 commits land since `cae8c30`).
- DroneOpsSync release v1.3.24: https://github.com/BigBill1418/DroneOpsSync/releases/tag/v1.3.24 — signer fingerprint identical to v1.3.23.

### Next
- Wait for `trig_01KiBK88vqs6vtRf75rkxcw8`; review + squash-merge PRs when they arrive.
- No backend changes land until those PRs merge. Current HEAD is production-stable.

<!-- ADR-0121 autosync verification probe 2026-06-13 — docs-only, no code impact -->
