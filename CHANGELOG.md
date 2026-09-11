> **Maintained automatically by NOC doc-autogen.** This file is refreshed twice daily (04:00 + 16:00 UTC) by `~/noc-master/scripts/doc-autogen.py`, which summarizes recent commits via Claude Haiku 4.5 and commits with a `[skip-deploy]` trailer so no container rebuilds are triggered. See [NOC-Master ADR-0013](https://github.com/BigBill1418/NOC-Master-Control-SWARM/blob/main/docs/decisions/ADR-0013-docs-only-deploy-skip.md). Manual edits are preserved — the generator diffs against existing content before writing.

# Changelog

Notable changes to DroneOpsCommand. Dates are absolute (YYYY-MM-DD, UTC).

## 2026-09-11 — P-EVAL: the `dji-log-parser` bump does not exist

FP-1's **P-EVAL** gate (ADR-0043 decision **D6**) is complete. Report:
`docs/reports/2026-09-11-dji-log-parser-upgrade-eval.md`. Harness:
`tools/p-eval/` — standalone, no DB writes, no schema change, not deployed, and
**`flight-parser/Cargo.toml` was not touched**.

**Verdict: do not adopt — there is nothing to adopt.** The newest published
`dji-log-parser` is **`0.5.7`**, which is exactly what `flight-parser/Cargo.lock`
already pins (confirmed by the crates.io API *and* `cargo search` under the
`rust:1.85-bookworm` toolchain the parser's Dockerfile pins). Upstream's last
release was 2025-04-26 and its repository has had one commit since — 15 months
dormant.

The only newer artefact is upstream `master` at `88fcfc96`, "Parse Inspire 1
battery serial numbers". Evaluated anyway, as the only candidate that exists:

- **782 comparisons over 762 distinct real DJI logs, 24 metrics each,
  6,756,743 `gps_track` coordinates: zero differences.** Bitwise, not
  within-tolerance. 759 DJI keychains fetched, 0 fetch failures, 0 parse errors,
  every log frame-decoded (all v14). Caveat stated in the report rather than
  rounded away: 19 of the 782 records decoded frames but never got a GPS fix, so
  their track comparison is vacuous — the 6,756,743 coordinates are across the
  763 records that have a track, and the header quantities those 19 fall back to
  are compared directly as 4 of the 24 metrics.
- Its only behavioural change is gated on `ProductType::Inspire1`/`Pro`/`RAW`.
  The fleet operates **no** Inspire 1, so it is unreachable here. Byte
  consumption is unchanged, so no field offset can shift.
- Adopting would swap a checksummed crates.io dependency for a git dependency on
  an unreleased commit, for no measurable gain.

**What this settles for the phases behind the gate:**

- **`Unknown(NNN)` does not resolve.** The `ProductType` enum is unchanged. All
  four placeholders persist — `Unknown(178)` Matrice 4TD, `Unknown(137)` Mavic
  4 Pro, `Unknown(139)` Mini 5 Pro, `Unknown(150)` Matrice 4T (the last appears
  only on recovered ODL-era logs and goes live at P7). 166 of 226 `dji_txt`
  flights — 73% — are on an airframe the crate cannot name. `dji.rs`'s
  `aircraft_name` fallback is permanent infrastructure, not a stopgap.
- **`SmartBatteryStatic` is not fixed upstream**, so **P2 must build the §2.4
  shim**. The plan's diagnosis is right and now sharper: every field after
  `index` is read exactly one byte early (consistent with C struct padding), so
  `raw >> 8` is correct and `swap_bytes()` is not. New limit: `>> 8` recovers the
  true value only while its top byte is zero — fine forever for
  `designed_capacity` and `full_voltage`, but **`loop_times` breaks at 256
  cycles**, and the plan's `0..=3000` plausibility gate cannot catch it (a
  260-cycle pack reads as 4).
- **P2 is unblocked and should proceed on `0.5.7`.**

**Corpus re-counted rather than inherited** (the plan's own §8 says to):
`/data/uploads/flight_logs` holds **200** files = **198** real DJI logs + the 2
dummies; 198 is exactly the hash-set intersection with `flights.source_file_hash`.
`dji_txt` rows are **226** (plan says 210; §8's correction says 218). 28 rows
still have no retained original. Plus the 584 recovered ODL-era originals, of
which 20 overlap the live set. The plan's 182/184/190/192 figures are all stale.

**Four follow-ups recorded in the report, deliberately not actioned here:**
`flight-parser/Cargo.toml` requests `"0.5"` not `"=0.5.7"` so the lockfile is the
only thing enforcing D6; `DJI_LOG_PARSER_VERSION` is a hand-maintained string
with nothing tying it to `Cargo.lock` despite being the provenance stamped on
every `flight_details` row; upstream dormancy makes `Unknown(NNN)` permanent;
and a fresh dependency resolve **no longer builds on `rust:1.85`** (the
`idna`→`icu` chain now wants 1.88), so the committed `Cargo.lock` is load-bearing
— never `cargo update` the parser without bumping its Dockerfile toolchain.

**Test output quoted, because nothing in CI runs these:**

```
tools/p-eval  cargo test   → test result: ok. 13 passed; 0 failed
tools/p-eval  --selftest   → selftest failures: 0
flight-parser cargo test   → test result: ok. 65 passed; 0 failed
```

## 2026-09-11 — docs sweep: CLAUDE.md and README told you to run a script that does not exist

A full pass over the repo's docs and metadata. Everything below was verified
against the working tree, not inherited.

**`CLAUDE.md` — four things were actively wrong**, and it is the file every
Claude Code session loads first:

- **Tech Stack said "Deploy: `update.sh` pulls latest, rebuilds changed
  services"**, and a *Server update commands* block listed `./update.sh`,
  `--clean` and `status`. `update.sh` was deleted in `e4610b5` — ADR-0018 records
  that removal, while this file kept advertising it. Replaced with the real path
  (fleet deployer on push to `main`), how to verify a deploy by what is running,
  and the `[skip-deploy]` semantics.
- **The version-bump list said "ALL 4 of these files"** and described
  `AppShell.tsx` as "the navbar footer", singular. It appears **twice** — desktop
  sidebar and mobile drawer — and the mobile one was repeatedly missed.
  `flight-parser/Cargo.toml` was absent entirely despite being the only thing
  that makes a parser deploy verifiable. Now 5 files, 6 locations.
- **`.deployer-disabled` was described in a way that reads as "auto-deploy is
  off."** Nothing in the fleet deployer reads that marker; this repo **is**
  continuously deployed on push to `main`. The real pause is
  `noc-master/data/soak-pause/<repo>.pause`.
- **Nothing warned that there is no test job in CI.** New *Tests & CI* section:
  no pytest or cargo job exists, so every "green" claim is local and
  hand-quoted; plus the three environment traps — `aiosqlite` (absent until
  2026-09-11, and its absence shows up as 29 ERRORs with a plausible-looking
  pass count), the OTLP endpoint that defaults to production Alloy when unset,
  and WeasyPrint's native libs.

**`README.md` — documented a broken setup path to self-hosters.** Two separate
blocks described `droneops-autopull.service`, `droneops-autopull.timer`,
`autopull.sh` and `tail -f autopull.log`, and offered a `--branch` flag. All of
those were removed with ADR-0018; `setup-server.sh` installs exactly **one** unit
(`droneops.service`) and takes no `--branch`. Anyone following the README on a
fresh install would have chased units that do not exist. Rewritten to the real
boot-start behaviour plus an honest *Updating* section, including why there is
deliberately no in-repo poller.

**Also:** `ROADMAP.md` still called FP-1 PLANNED with "nothing built" while P0+P1
were live (fixed in `c920ce8`), and the Tech Stack now names the Rust parser
service and the `flight_details`/`flight_series` sidecars with the ADR-0019
constraint that the flight-library list query must not touch them.

No code changed; no version bump.

## 2026-09-05 — v2.90.0: canonical DJI serials in the fleet matcher (ADR-0044)

88 production flights (49 Matrice 4TD + 39 Matrice 4T, all
`source = 'opendronelog_import'`) sat unattributed because DJI reports a
serial in two fixed-width forms and the matcher only understood one:

- **16-char header form** — `1581F8HGX255P00A`, what the DJI log header
  carries and what the parser emits.
- **20-char OpenDroneLog form** — the same serial plus a 4-char suffix,
  `1581F8HGX255P00A0FEK`.

`_match_fleet_aircraft()` compared with exact equality only, so a 20-char
flight serial never matched a 16-char aircraft row. The Matrice 4TD row
has existed since 2026-03-16 — this was never a missing-row problem.

**Change** (`backend/app/routers/flight_library.py`): a second pass
inside ADR-0007's serial branch. `_canonical_serial()` truncates serials
of *exactly* 20 characters to *exactly* 16 and leaves every other length
alone; both sides are canonicalized and compared for **full equality**.
It is a fixed-width truncation, not a prefix test — a truncated or
partial serial canonicalizes to itself and can never equal a 16-char
canonical, which is why this is safe where ADR-0007's banned model-name
prefix rule was not. 14-char DJI FPV serials carry no suffix and are
untouched.

Invariants preserved: exact equality always wins outright; a canonical
match resolves only when it selects exactly one aircraft (two or more →
unattributed + INFO log); a serial that is present but unmatched still
never falls through to model matching.

Also hardened: the exact-serial read moved from `scalar_one_or_none()`
(which *raises* on duplicate fleet serials — `aircraft.serial_number`
has no unique index) to `.scalars().all()`, so duplicates degrade to
ambiguity instead of an exception that would abort the whole startup
backfill. Aircraft rows with a NULL/blank serial are excluded from the
canonical pass.

**On first deploy the startup backfill will attribute 88 flights** — 49
to `DJI Matrice 4TD`, 39 to `DJI Matrice 4T` — and normalize their empty
`drone_model`. Both backfill paths remain scoped to
`aircraft_id IS NULL`; no operator-curated assignment is touched.

Tests: `backend/tests/test_flight_attribution.py` grew from 12 to 22
cases. Full backend suite at this commit (rebased onto FP-1 P0+P1):
`753 passed, 17 skipped in 268.46s`, up from `743 passed, 17 skipped` on
`34553cf` — exactly the 10 cases added here. `flight-parser` is untouched
by this change and its suite is unchanged: `cargo test` `65 passed; 0
failed`. Both run locally; this repo has no pytest or cargo CI job. No
schema change, no migration.

## 2026-09-05 — chore(tests): declare `aiosqlite` in `requirements-dev.txt`

Pre-existing test-infrastructure gap, present on `main` (`34553cf`) before
this branch — not introduced by FP-1 or by ADR-0044. Seven test modules
build SQLAlchemy engines on `sqlite+aiosqlite://`, but `aiosqlite` was
declared in neither `requirements.txt` nor `requirements-dev.txt`. It
happened to be installed in the environments where the suite had been run,
so nobody hit it; a clean-room
`pip install -r requirements.txt -r requirements-dev.txt` loses those
modules at *setup*, which pytest reports as ERROR rather than FAIL — a
quieter failure than a red test. There is no pytest job in CI to catch it.

Measured, not assumed. Same image, same commit, `aiosqlite` uninstalled:

```
724 passed, 17 skipped, 29 errors in 36.46s
```

and with it declared and installed:

```
753 passed, 17 skipped in 268.46s (0:04:28)
```

Exactly 29 tests across 7 modules were silently not running.

Pinned `aiosqlite==0.20.0` — the release contemporaneous with the pinned
`sqlalchemy[asyncio]==2.0.36` — in `requirements-dev.txt`, **not**
`requirements.txt`: production runs asyncpg against Postgres and must not
gain a SQLite driver. No runtime code changed and the prod image installs
`requirements.txt` only, so this cannot affect a deploy.

## 2026-09-05 — v2.83.0 / parser 1.2.0: FP-1 P1 — Tier 0 parser pass

The extended DJI-log data now actually gets extracted and stored. Everything
here comes from `log.frames()`, which the parser already iterates — **no second
decode and no second DJI keychain round-trip.**

**New `flight-parser/src/details.rs`.** A streaming accumulator riding the
existing frame loop, producing ~50 typed scalars, five JSONB groups and 15
full-resolution series per flight: time base, MSL/VPS altitude,
distance-from-home, vertical rate, aircraft and gimbal attitude, RC up/downlink,
battery current and cell-voltage deviation.

- **Full resolution at rest** (operator decision D2). Scalars are computed over
  every frame; series keep one value per frame. Reduction happens only at the
  API layer.
- **Per-quantity rounding** (§2.5) is what pays for it: 1 dp for metres, 2 for
  m/s, 3 for volts, 7 for lat/lon, and so on. `193.90000000000001` → `193.9`
  drops f64 mantissa noise and nothing else — every sample is kept and the
  stored text is ~4x smaller.
- **Missing samples are `null`, never `0.0`.** An RC link with no OFDM record
  yet, or a distance-from-home with no GPS fix, stores a gap. A `0` there would
  read as "signal lost" / "at the home point" — a different and alarming claim
  about the flight.
- **Integrals use the real inter-frame interval**, so `battery_energy_wh` is
  invariant to log rate. A frame-count-times-assumed-cadence integration is the
  ADR-0027 mistake in a new place.
- **Event extraction** (§2.6): garbled prefixes trimmed and flagged, dedupe on
  the cleaned string (the census's 18 identical "Remote controller
  disconnected" strings collapse to one record with `count: 18`), and a
  remnant under 8 characters becomes `kind: "unparsed"` rather than a guess.
- **Time-base provenance.** `FrameCustom::default()` is the Unix epoch, so a
  log with no `Custom` records would stamp 1970 on every sample. The base is
  chosen explicitly — wall clock, else DJI's own `fly_time` counter, else none
  — and recorded in `config.time_base`.

**Three long-empty fields now carry data.** `TelemetryData.timestamps`,
`signal_strength` and `distance_from_home` were hard-coded empty/`None`, which
is why `/telemetry` has always served `signal_strength: null`. They go from
null to arrays; the frontend already types them permissively.
`TrackPoint.timestamp` is populated when the log carries a real clock.

**`Unknown(NNN)` → `aircraft_name` fallback.** The crate's `ProductType` enum
predates the Mavic 4 Pro and Matrice 4 series, so those airframes render as the
literal `Unknown(178)` — 150 of 210 stored DJI flights carry it. New imports
now fall back to the header's `aircraft_name`. The predicate is anchored on
both ends, so a real model name can never be displaced.

**Litchi / Airdata are untouched.** `details` is a struct-literal field, so
adding it was a compile error in both until each declared `details: None` — the
compiler enforced the audit. `skip_serializing_if` omits the key entirely, so
their JSON output is byte-identical; both have a test asserting it.

**Backend persistence.** `app/services/flight_details_writer.py`, called from
`_build_flight_from_parsed` inside the **same best-effort savepoint pattern**
battery tracking already uses. A details failure costs a WARN line and nothing
else — the flight record is the operator's work product; extended data is not.
The payload crosses a service boundary, so every value is coerced against its
column: integers range-checked against their SQL width, strings truncated,
NaN/inf and uncoercible values dropped to NULL, series bounded and deduped on
the primary key, and `sample_count` recomputed rather than trusted.

**A cross-language wire fixture.** `backend/tests/fixtures/parser_details_payload.json`
is generated by the Rust suite (`cargo test emit_wire_fixture`) and asserted
against by both sides. This exists because the silent failure mode of a JSON
boundary between two languages is: rename a field on one side, every column
writes NULL, and the import still logs success. Verified by deliberately
drifting the fixture — `photo_count` → `photoCount` turned three tests red,
including `assert None == 2` on the stored row.

Test output quoted in `PROGRESS.md`: `cargo test` 65 passed; `pytest` 759
passed / 1 skipped with a live Postgres.

## 2026-09-05 — v2.82.0: FP-1 P0 — flight-details schema + read path (inert)

First phase of FP-1 (ADR-0043, plan
`docs/plans/2026-09-04-flight-details-data-ingestion.md`). **Nothing writes to
the new tables yet and no existing behaviour changes** — the schema and the
read surface land first so the parser phase has somewhere to put data.

**Schema — two sidecar tables, not more columns on `flights`.**
`flights` is the subject of three OOM ADRs and already carries three heavy
JSON columns; widening it would make every `select(Flight)` heavier, protected
only by remembering to `defer()`. Migration `0010_flight_details` creates
`flight_details` (1:1, ~76 typed scalars + eight JSONB groups) and
`flight_series` (one row per named series, PK `(flight_id, source, name)`).
Migration `0011_battery_src_truth` adds three nullable battery columns
(`batteries.cycle_count_observed`, `batteries.metrics_source`,
`battery_logs.pack_cycle_count`) one phase early, so the phase that actually
switches battery semantics needs no migration of its own. Both migrations are
idempotent per ADR-0042 — `0001` builds fresh databases from the live models,
so on a fresh install both must no-op instead of raising.

`Flight.details` / `Flight.series` are declared `lazy="noload"`. That is
load-bearing: `selectin` would join the sidecars into every `select(Flight)`
including the 500-row mission-picker query, which is ADR-0019's production OOM
with a larger payload.

**Read path.** `GET /{id}/details`, `GET /{id}/details/series`,
`GET /details/status`. `/details` never 404s on a missing row — per operator
decision D3 the link renders on every flight, so "no extended data" is a
normal response carrying an `unavailable_reason` (`source_unsupported` /
`not_backfilled` / `odl_import_no_original`). The flight row is read
column-explicitly and the series index selects every column except `values`.

**`downsample` extracted** from the closure inside `get_telemetry` to
`app/services/telemetry_downsample.py`, now shared by `/telemetry` and
`/details/series`. ADR-0032's own conclusion is that the absence of a shared
layer is what lets a unit-defect class recur; a second copy-pasted
downsampler would be the same mistake. Behaviour is pinned identical to the
old closure by a parity sweep over 60 (length, target) pairs. **One
deliberate difference:** `max_points=1` used to raise `ZeroDivisionError` (a
500 from a query string clients are free to send, since `/telemetry` declares
no lower bound); it now returns the first sample.

**Report-audience guard (ADR-0043 §4.4 / D1).** Operator decision D1 stores
the pilot's raw position track. `reports.REPORT_READABLE_DETAIL_FIELDS` is an
empty allowlist documenting that nothing from the details surface is
report-eligible, and a guard test asserts the six client-artifact-producing
modules reference neither table, plus a runtime test that the CSV/GPX/KML
exporters emit no pilot fields. ADR-0029 unchanged: no altitude is compared
to any limit anywhere in this work.

**Encoding measured, not argued (plan §1.5 / C-2).** One flight's series
written both ways into scratch tables on a real `postgres:16-alpine`, 13,870
samples: `json` 99,012 B vs `float8[]` 109,896 B vs `jsonb` 125,376 B, and
`json` read+parse 4.34 ms vs `float8[]` 9.43 ms. **`json` confirmed** — it is
both smaller and ~2.2x faster, overturning the plan's speculation that a
native float array might read faster. Numbers in `PROGRESS.md`.

Tests: +113 (`729 passed, 1 skipped` with a live Postgres; `723 passed,
7 skipped` hermetic). Includes a real-Postgres tier covering the *production*
upgrade path — an existing DB stamped at 0009 with the objects absent, which
is the only path that executes the `create_table` / `add_column` bodies at
all; the pre-existing fresh-DB tier only ever exercises the idempotency
guards.

## 2026-09-01 — v2.81.0: six new billable-rate templates

Operator-directed expansion of the seeded billable rates (PV/solar
inspection service line):

- **PV Thermal Inspection — Field Day** — Billed Time, $2,200.00 flat
- **Mobilization — Regional Overnight** — Travel, $850.00 flat
- **Lodging + Per Diem** — Travel, $235.00 per day
- **Weather Standby** — Billed Time, $1,100.00 flat
- **Data Processing & QA** — Billed Time, $150.00/hr
- **Third-Party Analytics (pass-through)** — Other, billed at vendor
  cost with **no markup** (operator decision 2026-09-01; earlier +15%
  and +10% drafts both rejected). The schema has no formula field, so
  the template is seeded at $0.00 with the rule in its description —
  the invoicing operator enters the vendor amount as the unit price.

Mechanics: `backend/app/seed.py` rate templates hoisted to a module-level
`RATE_TEMPLATE_SEED` constant (same pattern as `AIRCRAFT_SEED`); the
startup seed inserts by name only when missing, so existing prod rows
(including any operator edits to the original eight) are untouched. New
hermetic test `backend/tests/test_rate_template_seed.py` (9 tests) locks
the list. No schema change, no migration.

## 2026-08-23 — v2.80.4: startup failures are loud + ADR-0042

Closes the last residual from the 2026-08-22 fresh-install audit. Root
cause of the silence, found by reproducing the failure in a scratch
container: `alembic/env.py` ran `fileConfig(alembic.ini)` on every
invocation, and `fileConfig()` defaults to `disable_existing_loggers=True`
— so the moment `command.upgrade()` ran during startup, the app's `doc`
logger AND `uvicorn.error` were disabled, and every subsequent message
(including the exception that explained the crash and uvicorn's
"Application startup failed") was dropped. That is why the seven-week
fresh-install crash loop (v2.80.2) restarted every ~5 s with logs that
simply ended mid-migration.

- `alembic/env.py`: `fileConfig()` now runs only for the standalone CLI —
  skipped when the programmatic path passes its connection via
  `config.attributes` — so app logging survives migrations.
- `app/main.py`: the entire pre-yield lifespan body is wrapped; on any
  exception the full traceback is logged (`STARTUP FAILED — …`) and also
  printed directly to stderr (immune to any future logging reconfig),
  then re-raised.
- Verified: forced a bogus `alembic_version` revision in a scratch
  container — exit 3 as before, but `docker logs` now carries the full
  traceback ending in the real error.

New **ADR-0042** (`docs/adr/0042-fresh-install-integrity-and-demo-hygiene.md`)
records the whole 2026-08-22 incident and the standing decisions:
post-baseline migrations must be idempotent, startup failures must be
loud, the demo resets nightly (BOS crontab + `scripts/demo-nightly-reset.sh`,
ntfy on failure), and no realistic random literals in tests.

## 2026-08-22 — v2.80.3: green Secret Scan + nightly demo reset

Two "big-time readiness" items from the same audit that produced v2.80.2:

- **Secret Scan CI red on every push since the ToS tests landed** — gitleaks
  flagged the realistic random `intake_token` literal in
  `backend/tests/test_tos_accept_route_body.py` (generic-api-key,
  entropy 4.9). It was a test-only round-trip value with no format
  constraint beyond `max_length=64`, so it is now an obviously-fake
  low-entropy stand-in (`TESTONLY-…`) instead of an allowlist entry —
  no weakening of the gate, robust to line-number drift, and the workflow
  goes green. A permanently-red public workflow both looks broken and
  trains everyone to ignore the one gate that matters.
- **`scripts/demo-nightly-reset.sh`** — the demo instance accumulated
  months of visitor junk (uploaded flight logs with real GPS, third-party
  contact emails) because `DEMO_RESET_INTERVAL_HOURS` is configured but
  unimplemented. Until an in-backend reset task exists (celery beat is not
  an option — the demo worker/beat must stay stopped, dunning-email
  hazard), this script is the reset: wipe the demo schema, restart the
  backend (startup rebuilds via alembic + demo seed), wait for healthy,
  verify the trial login end-to-end, and page `droneops-demo-reset` (high)
  via the ADR-0036 helper on any failure. Installed in the BOS-HQ operator
  crontab at 09:23 UTC (2:23 AM PDT) daily, following the existing
  snapshot-cron pattern.

## 2026-08-22 — v2.80.2: fix fresh-install crash loop in migrations 0008/0009

Every fresh database — the demo instance reseed, the self-host Quick Start,
and future managed-client provisioning — crash-looped at startup since
0008 landed (2026-07-05). Root cause: `0001_baseline_schema` builds fresh
DBs with `Base.metadata.create_all` from the **live** models, which already
include the columns 0008/0009 add, so their bare `op.add_column` raised
`DuplicateColumn`. Existing databases (prod) never noticed: they are stamped
past the baseline and their columns were added by the ORM path before the
migrations existed.

The failure was silent: uvicorn exits 3 on lifespan-startup failure and the
migration exception's traceback never reached the container logs — the demo
just looped every ~5 s. Diagnosed by running `alembic upgrade head` in a
one-off container, which surfaced the real `DuplicateColumn`.

- `0008_report_dl_payment_override` and `0009_mission_dl_email_sent_at` now
  no-op when their column already exists (inspector guard), matching the
  idempotent house style of 0001–0007.
- **Rule going forward:** every post-baseline schema migration must be
  written idempotently, because 0001 always produces the current-model
  schema on fresh DBs. A bare `op.add_column`/`op.create_table` WILL break
  fresh installs while passing on every existing DB and in CI against
  migrated schemas.
- Known residual: the swallowed startup traceback (uvicorn exit 3 with no
  logged exception) made this a forensic hunt; surfacing lifespan failures
  in logs is a candidate follow-up.

## 2026-08-19 — cloudflared 2026.3.0 -> 2026.8.2 (fleet-wide version-rot remediation)

This stack's tunnel connector was running cloudflared 2026.3.0. Cloudflare
supports releases "within one year of the most recent release"; a 2026-08-19
fleet survey found 22 connectors, most months behind, and one (the ntfy alerting
tunnel) already outside that window.

It rots silently by construction: the official cloudflare/cloudflared Docker
image disables the built-in self-updater, a floating `:latest` tag only resolves
at container recreate, and the NOC deployer reacts to git commits rather than
upstream image releases. Nothing was ever going to notice.

Image tag only — no functional, ingress, auth or routing change. Fleet-wide
reasoning, survey, alternatives considered and the monthly supervised bump
routine: `noc-master/docs/adr/0212-cloudflared-version-rot.md`.

## 2026-08-18 — ops(standby): BK-2 executed — standby archive_mode off, 1.5 GiB stale WAL reclaimed [skip-deploy]

The droneops standby on svdp-dev (`droneops-db-standby`) carried inherited
`archive_mode='on'` + the Gap-7 `archive_command` in `postgresql.auto.conf`
(ALTER SYSTEM is blocked in recovery, so the file was edited directly), plus
**1.5 GiB / 99 stale WAL segments** in its own `wal_archive` from the seeding
basebackup. Fixed and deleted; two standby restarts (first attempt targeted
`postgresql.conf`, where the setting does not live — `pg_settings.sourcefile`
pointed to `auto.conf`). Verified: standby in recovery, `archive_mode=off`,
wal receiver `streaming`, primary slot active, `replay_lag` empty, pgdata
2.3 G → 851 M. Production primary untouched. A promotion can no longer
recreate Gap 7. ROADMAP BK-2 closed.

## 2026-08-18 — ops(backups): yearly retention → unlimited (operator decision) [skip-deploy]

Bill: "retention is indefinite." `KEEP_YEARLY` 7 → `unlimited` in
`droneops-backup.sh`; yearly backup snapshots are never pruned (ADR-0041 D4
amended). Backup history only — live flight data was never subject to any
retention. Daily/weekly/monthly tiers unchanged (14/8/24).

## 2026-08-18 — ops(volumes): orphaned legacy volumes archived + removed [skip-deploy]

Operator-approved disposal of the two truly orphaned volumes on BOS-HQ
(referenced by zero containers and zero compose files, verified before
touching):

- **`droneops_postgres_data`** (46 MB) — the pre-promotion BOS primary pgdata,
  superseded when `droneops-standby-db` was promoted. Archived first into the
  encrypted restic repo as a tar (tag `legacy-bos-primary-pgdata`, snapshot
  `66ed2135`, restore-read verified: 1,204 entries incl. `PG_VERSION`), then
  removed.
- **`droneops-demo_ollama_data`** (4 KB, empty; demo compose has ollama
  disabled) — removed, nothing to archive.

**The demo stack was NOT touched** — it is live and tunnel-exposed (6 healthy
containers up 2 weeks); its data volumes are in active use and are not
"legacy". `droneops-gw_caddy_data` (live gateway ACME state) also untouched.
Post-check: 103 containers running, demo 6/6 healthy, prod DB accepting
connections.

## 2026-08-18 — ops(backups): §5.7 cutover automated — one-shot gated timer on droneops-server [skip-deploy]

Operator approved executing the cutover without waiting for a live session.
New `scripts/droneops-backup-cutover.sh` runs once via a user-level systemd
timer on droneops-server (HSH-HQ — the host with both BOS ssh access and repo
push credentials; HSH's `/etc` is bind-mounted read-only, so user units are
the local pattern) at **2026-08-20 04:12 UTC**, after the soak window's final
run. Fail-closed: all four gates re-verified over ssh before any mutation, any
failure aborts pre-mutation and pages `infrawatch-alerts` at `high`; success
notifies at `default`. Dry-run validated under the systemd user environment on
2026-08-18 (refused correctly on the not-yet-met ≥4-snapshot gate, ntfy
suppressed). The script self-documents the executed cutover into PROGRESS.md
and pushes.

## 2026-08-17 — ops(backups): legacy n8n final state archived, HSH stale dumps retired [skip-deploy]

Operator-approved cleanup of the retired HSH-HQ backup lane (`~/backups/` on
droneops-server, dead since 2026-04-15). The final n8n SQLite dump
(`n8n_20260415_020001.sqlite`, 218 MB, sha256 `ae28fe7b…`) — the last surviving
data from n8n, decommissioned fleet-wide 2026-04-21 — was archived into this
repo's encrypted R2 restic repository under its own tag **`legacy-n8n`**
(snapshot `1d0cfb76`, 19 MiB stored), restore-verified byte-identical, and all
local copies deleted (~994 MB reclaimed, dirs removed). Not a DroneOps
artifact; parked here because this is the fleet's encrypted archive repo — see
the lane table in `docs/runbooks/droneops-backup-restore.md`. The retirement
README at droneops-server `~/backups/README.RETIRED.md` records the disposition.

## 2026-08-17 — ops(backups): cold DR rehearsal + four review defects (ADR-0041 as-built) [skip-deploy]

Adversarial re-review of the backup lane shipped earlier the same day
(`e43018f`), plus the **first cold disaster-recovery rehearsal**: a full
rebuild from the **1Password Fleet items and the R2 bucket only**, executed on
`droneops-server`, with nothing read from BOS-HQ except comparison hashes and
no production container or volume touched.

**The rehearsal passed.** Recovery works cold. Three independent paths — restic
from R2, the plain break-glass `.sql.gz`, and live production — produced
**identical content digests**: `flights_digest=4d6d9276…`, 146,420,719 bytes of
telemetry, 334,757,775 bytes of GPS track, 6,842,636 telemetry points. All 226
files in the `files` lane are sha256-identical to production, and all 10
executed TOS PDFs match `tos_acceptances.signed_sha256` — a cross-lane proof
that the db and files lanes agree with each other. The restored `.env` matches
live byte-for-byte, and the full 11-service stack renders from restored config
alone; with `.env` removed the same render is correctly *refused* on the
ADR-0012 `:?` guard, so that check is not vacuous. Full evidence table:
`docs/runbooks/droneops-backup-restore.md` §11.

Four defects found and fixed (`c3d9502`) — none had broken a restore, but each
could have:

* **No concurrency guard.** The runbook tells operators to run the backup by
  hand; systemd blocks a second *service* start but not a manual shell run
  overlapping a timer run. Both would contend for restic's exclusive lock
  during `forget --prune`, turning a benign overlap into a `high` page. Added
  `flock`; an overlap exits 0 *without* stamping the freshness metric, so a
  one-off overlap is silent while a persistent one is still caught within 28 h.
* **`backups/` was not gitignored** — 1.1 GB of *plaintext* pg dumps (customer
  PII, invoice records, `device_api_keys`) sitting untracked in the deploy
  clone's working tree, one `git add -A` from being committed. Now ignored.
* **The quarterly drill never read the `files` lane** — 657 MiB of flight logs,
  report deliverables and executed TOS PDFs, the largest lane. It certified
  "restorable" while never touching it. Now asserted via a cross-lane sha256
  check rather than a file-count floor, which would stay green against a stale
  or truncated snapshot.
* **Post-metric error hole.** The local retention sweep ran after the freshness
  stamp under `set -e` with no `|| fail`, so a failure exited non-zero with no
  ntfy and a green metric — visible only in systemd.

Also fixed: **Procedure A2's first database command did not work.**
`docker compose up -d droneops-standby-db` fails with `no such service` — the
service is `db-standby`; `droneops-standby-db` is the *container* name. This
was in the critical path of a from-nothing recovery.

Explicitly re-verified and **not** changed: `forget --group-by tags` retention
is correct. A 40-day, twice-daily synthetic corpus (80 snapshots) converged to
exactly 20 — 14 daily + weeklies + monthly, one per day. The two same-day
snapshots per lane currently visible in R2 are a transient artifact of a
one-day-old repository, not a policy fault. Bash *does* run the `EXIT` trap on
`SIGTERM` (tested), so the workdir holding the repository password is not
leaked on a systemd timeout. `fail()` does **not** fail open when ntfy is
absent — exit 1 confirmed with the helper removed.

Minor, recorded not fixed: `.env` carries `FRONTEND_URL` twice (42 assignments,
41 unique keys); `uploads/tos_signed/` holds 11 PDFs against 10
`tos_acceptances` rows (one orphan from an abandoned signing flow, correctly
backed up).

## 2026-08-17 — ops(backups): comprehensive ENCRYPTED backup to R2 (ADR-0041) [skip-deploy]

Ops-scripts + docs only (no app change, no version bump). Closes the seven gaps
in the backup lane. The old plaintext lane is **still running in parallel** and
is not removed by this change — cutover is gated on three green days
(`PROGRESS.md`).

* **`scripts/droneops-backup.sh`** (new, supersedes `snapshot.sh`) — four
  restic lanes into a **dedicated, encrypted** R2 repository: `db`
  (`pg_dump -Fc`), `files` (`uploads/` **+ `reports/`**), `config`, and a
  one-shot `legacy`. Fail-closed on every path; the freshness metric is
  stamped only after all lanes, the retention pass and the integrity check
  succeed.
* **Encryption + a dedicated credential.** New R2 bucket `droneops-backups`
  with a **bucket-scoped** token replaces the account-wide
  `OBS_GLITCHTIP_BACKUPS_R2_*` reuse — blast radius drops from four services
  to one. Customer PII, invoice records, `device_api_keys` and the 11 executed
  TOS PDFs are no longer stored in the clear. `RESTIC_PASSWORD` and the R2
  credentials are filed to the 1Password **Fleet** vault (ADR-0086); the
  password is the recovery key and is unrecoverable if lost.
* **Host config is finally backed up** — `.env` (41 keys incl.
  `JWT_SECRET_KEY`, `POSTGRES_PASSWORD`, `CLOUDFLARE_TUNNEL_TOKEN`) and the
  compose overrides. Restoring DB + files without these produced a stack that
  *could not boot*. The lane is an explicit allowlist, so rotated `.env.bak-*`
  secrets cannot be swept in.
* **`uploads/` gains real history.** `aws s3 sync` was an additive mirror that
  faithfully copied corruption over the only good copy; snapshots make a
  damaged flight log recoverable.
* **Retention is now enforced in R2** — `forget --prune`
  14d/8w/24m/7y `--group-by tags`, replacing a sweep that pruned only the local
  copy while R2 grew unbounded at ~54 MB/day.
* **Dedup verified, not assumed.** The dump is fed to restic **uncompressed**
  (`-Z0`); a second full run added **414 KiB** (`files` lane: 0 B). Compressing
  first would have stored a fresh ~55 MB nightly forever.
* **WAL archiving retired.** `archive_mode=off` + `wal_archive/` deleted:
  it wrote into the very volume it protected, had never been pruned
  (5.5 GiB / 358 segments), and had not archived since 2026-07-22 — a 26-day
  hole. **Reclaimed 5.5 GiB** (`droneops_standby_pgdata` 8.0G → 2.5G); the
  `chad_hq_standby` slot stayed `active` throughout. Plus 68 MB of stale
  in-app dumps and ~130 MB of dead pre-migration dumps on droneops-server
  (archived to the `legacy` lane first).
* **`scripts/restore-drill.sh`** — migrated to restic (`pg_restore`, not
  `gunzip | psql`). Preserves the 90 %-of-live `flights` ratio, the <48 h
  freshness assertion, the throwaway DB and its `trap` cleanup. **Adds a
  `config`-lane assertion**: `.env` is restored and sha256-compared against the
  live file — the only check that proves the critical gap stays closed.
* **`scripts/systemd/droneops-backup.{service,timer}`** — twice daily,
  03:23 + 15:23 UTC (RPO 24 h → **12 h**), `Persistent=true`. UTC deliberately:
  the BOS-HQ nightly window is stacked in UTC and a local-time entry would
  DST-collide with a sibling job twice a year.
* **The failure path was observed firing**, not assumed: a deliberately
  corrupted R2 secret produced exit 1 and one `high` ntfy on
  `infrawatch-alerts` with the previously-missing click URL
  (`noc-mastercontrol.barnardhq.com/status/droneops`, HTTP 200) — and exactly
  one notification across two failures, confirming the 6 h cooldown.
* **Metric names deliberately unchanged.**
  `droneops_backup_last_success_timestamp_seconds` and
  `droneops_restore_drill_last_success_timestamp_seconds` are a hard contract
  with two live Grafana rules; renaming either would have converted a live
  alert into a permanently-green dead man.

New runbook: `docs/runbooks/droneops-backup-restore.md` (full DR, DB-only
rollback, single-file recovery, break-glass without the restic password, and
how to run the drill). Decision record: `docs/adr/0041-*.md`.

## 2026-08-05 — ops(data): prod maintenance-alert clear + 30→90-day interval tune [skip-deploy]

Data-only change in the prod DB (no code, no schema, no version bump). On
Bill's direction, in three passes:

* **Schedule-based clear:** reset `last_performed` to 2026-08-05 on all 8
  overdue `maintenance_schedules` (same semantics as the app's Skip/Defer
  endpoints), extended all nine 30-day `interval_days` to 90 (Sensor
  Cleaning / Battery Health Check / Firmware Review across Avata 2 /
  Matrice 30T / Matrice 4TD), and deferred the two Matrice 4TD 90-day items
  inside the due-soon window (IMU Calibration, Remote Controller Inspection).
* **Correction — record-based alerts:** the above did NOT clear the dashboard;
  `GET /maintenance/due` has a second alert source, `maintenance_records.next_due_date`.
  Four March-2026 service records carried stale due dates (up to 82 d overdue,
  on aircraft with no schedules at all — Mini 5 Pro, Mavic 3 Pro, plus
  Matrice 30T and Avata 2). Set `next_due_date = NULL` on those 4 rows
  (service history retained).
* **Final verified state:** all four dashboard alert sources at zero
  (schedule date-based, schedule never-performed, record `next_due_date`,
  battery health/cycles). Next date-based due item: Avata 2 Gimbal
  Calibration 2026-10-04.

Details, the schedules-JOIN verification blindness, and the seed-defaults
re-seed gotcha: `docs/ops/2026-08-05-prod-maintenance-interval-tune.md`.

## 2026-07-22 — ops(backups): automated quarterly R2 restore drill [skip-deploy]

Ops-script only (no app/version change). Closes the last discipline gap in the
backup lane: the quarterly restore drill documented in `scripts/snapshot.sh`
was manual — it relied on an operator remembering every ~92 days.

* **`scripts/restore-drill.sh`** — downloads the *newest R2 dump* (the off-host
  copy that matters in a disaster, not the local file), verifies gzip integrity
  and that the dump is <48 h old, restores it into a throwaway
  `droneops_restore_drill` database on `droneops-standby-db`, sanity-checks
  restored row counts (`flights` ≥90 % of live, `battery_logs` /
  `tos_acceptances` non-empty), then drops the scratch DB (trap-guaranteed).
* **`scripts/systemd/droneops-restore-drill.{service,timer}`** — installed on
  BOS-HQ at `/etc/systemd/system/`, `OnCalendar=*-01,04,07,10-16 16:23 UTC`
  (quarterly on the 16th, anchored to the 2026-07-16 install-time verified
  restore), `Persistent=true` so a powered-off host catches up.
* **Self-watching:** full success writes node-exporter textfile metric
  `droneops_restore_drill_last_success_timestamp_seconds`; an InfraWatch
  Grafana rule pages `infrawatch-alerts` if the drill has not succeeded in
  >100 days or the metric is absent. Failure fires an ntfy `high`
  (dedup `droneops-restore-drill`, 6 h cooldown); success posts one
  `default`-priority note (4×/year, ADR-0037 digest class).
* **Proved live 2026-07-22:** restored `droneops/db/2026/07/22/…sql.gz` from
  R2, verified flights=760/760, battery_logs=759, tos_acceptances=10; scratch
  DB dropped; metric stamped.


## 2026-07-19 — ops(standby): silence chronic healthcheck FATAL spam on droneops-db-standby [skip-deploy]

Compose-only (no app/version change). The standby's healthcheck ran
`pg_isready -U replicator`; dbname defaults to the username and no
`replicator` database exists, so PostgreSQL logged
`FATAL: database "replicator" does not exist` every 10 s (~8.5k lines/day)
while the check still passed. Healthcheck now probes `-U droneops -d droneops`
(the app role+db, present on the standby via replication). Applied live on
CHAD-HQ (10.99.0.2) by recreating `droneops-db-standby`.

## 2026-07-16 — ops(backups): off-host R2 push + fix broken tos_signed path + freshness metric [skip-deploy]

Ops-script only (no app/version change). The 2026-07-16 BOS backup audit found
the nightly `scripts/snapshot.sh` dump was **local-only** (no off-host copy)
and its signed-TOS step was silently no-op'ing every night — it tarred
`${REPO_ROOT}/data/tos_signed`, a path that never existed. The real signed
legal PDFs live in the `droneops_app_data` Docker volume at `uploads/tos_signed`
(alongside `uploads/flight_logs`, ~557M total).

* **Off-host DB push.** After the local gzipped `pg_dump` (unchanged, 14-day
  local retention), the dump is streamed to Cloudflare R2 at
  `s3://<obs bucket>/droneops/db/YYYY/MM/DD/droneops-<TS>.sql.gz`, reusing the
  obs R2 credential source (`/opt/observability/.env`) and the shared
  `obs-glitchtip-backups` bucket with a dedicated `droneops/` prefix.
* **Fixed + expanded uploads coverage.** Replaced the broken `data/tos_signed`
  tar with an incremental `aws s3 sync` of the volume's `uploads/` tree
  (signed-TOS PDFs + flight logs) to `s3://<obs bucket>/droneops/uploads/`,
  mounting the volume read-only.
* **Freshness metric + alerting.** On FULL success writes
  `droneops_backup_last_success_timestamp_seconds` to the node-exporter
  textfile collector; InfraWatch `obs-rule-droneops-backup-stale` pages on
  >28h/never-written. Any dump/upload failure pushes ntfy `high` to the
  existing `infrawatch-alerts` topic. Removed the silent `|| true` swallow.

## 2026-07-06 — fix(payments): delivery verification pass — two real bugs + e2e endpoint tests — v2.80.1 (ADR-0040 addendum)

End-to-end verification of the v2.80.0 automation caught two bugs before any
prod payment exercised them (full detail in the ADR-0040 addendum):

* **`Mission.invoice` lazy="noload" identity-map trap.** Every trigger path
  loads the mission before the delivery service runs, so the service's
  `selectinload(Mission.invoice)` re-query returned the identity-mapped
  mission WITHOUT repopulating the relationship — the gate read
  `invoice=None` and skipped `not-paid-in-full` on PAID missions. The Stripe
  webhook and mission-update triggers were silently dead. Fix: the service
  queries the Invoice table directly; `_delivery_skip_reason(mission,
  invoice)` takes it explicitly.
* **SMTP-unconfigured no-op was stamped as sent.** `_send_html_email`
  returns False when SMTP isn't configured; the stamp was written anyway,
  permanently losing the delivery. Fix: stamp only on a True send; the False
  path returns `skipped:smtp-unconfigured` (WARN) and stays armed.
* New `test_download_link_delivery_e2e.py`: 9 endpoint-level tests driving
  the REAL `update_invoice` / `update_mission` / `get_client_mission`
  functions against sqlite (house pattern; includes a JSONB→JSON sqlite
  shim for the reports table). These are the tests that caught bug #1.
* **Report-editor override could be silently lost (independent review
  finding).** Generate Report / Generate PDF re-baselined the unsaved
  `paymentOverride` switch without persisting it — the dirty-guard went
  quiet and the server kept override=false, so the operator believed the
  link was released while PDF + email withheld it. Fix: the pre-PDF PUT now
  persists `include_download_link` + `download_link_payment_override` (the
  PDF renders against what the operator sees), and the generate paths
  preserve the previous baseline so an unsaved flip stays dirty.
* Bypass sweep confirmed: portal + report PDF/email are the only
  client-reachable `download_link_url` surfaces, all gated; docs updated
  (README feature sections, PROGRESS, ADR-0040 addendum).
* Suites: 607 backend / 53 frontend pass; tsc clean.

## 2026-07-06 — feat(payments): automated download-link delivery on payment-in-full — v2.80.0 (ADR-0040)

Completes ADR-0039: payment-in-full is now a TRIGGER, not just a gate. Per
Bill (2026-07-06): no manual report regeneration — when the client pays they
get the link in a separate automated follow-up email and it populates in the
client portal.

* **Delivery service** `app/services/download_link_delivery.py` — also now
  owns the ADR-0039 gate policy (reports router + portal import it; one
  source of truth). Sends branded `download_link_email.html`, stamps
  `missions.download_link_email_sent_at` (migration
  `0009_mission_dl_email_sent_at`) AFTER a successful send so failures retry
  on the next trigger. Fail-soft: never breaks the payment flow.
* **Three triggers:** Stripe balance-paid webhook; manual mark-paid
  (`PUT /invoice` false→true transition); download URL set/changed on the
  mission (covers footage-ready-after-payment; a URL change RESETS the dedup
  stamp so replacement links re-deliver).
* **Client portal:** `GET /api/client/missions/{id}` returns
  `download_url`/`download_expires_at` only when the gate passes; the
  DELIVERABLES card shows the download button when unlocked ("unlocks when
  the invoice is paid in full" while unpaid), and the post-payment poll
  re-pulls the mission so the link appears without a reload. Gotcha honored:
  `Mission.invoice` is lazy="noload" — endpoint eager-loads it explicitly.
* Skip conditions logged with reason: no-url / already-sent / not-billable /
  not-paid-in-full / link-expired (WARN) / no-customer-email (WARN).
* Tests: 16 new (`test_download_link_delivery.py`); portal fixture +
  migration fence updated; 598 backend / 53 frontend pass.

## 2026-07-05 — feat(reports): unpaid-invoice download-link gate + operator override — v2.79.0 (ADR-0039)

Policy (Bill, 2026-07-05): **clients do not get the mission-footage download
link until the invoice is paid in full.** Trigger: the 2026-07-02 River M.
report went out with the footage link while BARNARDHQ-2026-0005 ($400.50) was
unpaid — nothing in the code checked payment.

* **Server-side gate at a single choke point.** Both exposure paths (report
  PDF render + report email) now build the link only via
  `_build_download_link()` → `_download_link_payment_blocked()`
  (`backend/app/routers/reports.py`). Withholds while a billable mission's
  invoice is unpaid; **fail-closed** when billable-but-never-invoiced; $0
  invoices and non-billable missions pass. Deposit alone does NOT release —
  only `paid_in_full`.
* **Per-report operator override** `reports.download_link_payment_override`
  (migration `0008_report_dl_payment_override`, additive, default false).
  Settable only via `PUT /report` (never the generate path, so regeneration
  can't reset it); every flip is audit-logged with the acting user.
* **Editor surface** (`MissionReportEdit.tsx`): yellow "link withheld —
  invoice not paid in full" alert + orange override switch when the link is
  requested and payment is outstanding; the Sent toast says explicitly when
  the link was withheld (`download_link_withheld` on the send response).
  `GET/PUT /report` return computed `download_link_payment_blocked`.
* **Withholding never blocks the report itself** — the client still gets the
  report; only the footage link is held.
* **Residual:** a PDF rendered pre-gate carries the baked-in link; send
  warn-logs this and River's stale `pdf_path` was invalidated in prod. See
  ADR-0039 for the full policy + alternatives.
* Tests: 14 new gate tests (`test_report_download_link_payment_gate.py`);
  migration-fence + ADR-0038 fixtures updated; 582 backend / 53 frontend pass.

## 2026-07-03 — feat(reports): client-report narrative quality levers — v2.77.0 (ADR-0035)

Guard-safe quality pass on the shared report system prompt
(`SYSTEM_PROMPT_TEMPLATE` in `backend/app/services/ollama.py`, inherited by the
Claude path via `claude_llm.py`). Implements the top three levers of
**docs/plans/2026-07-03-report-quality.md** (`FU-AI-QUALITY-PASS`):

* **Kill hedging (§3.1).** Requires definitive, active-voice authority; forbids
  "appeared to" / "seemed" / "was observed to" / "it is likely" softeners unless
  the data is genuinely uncertain.
* **Anti-bloat budget (§3.2).** Each section is 2–5 sentences of substance — no
  padding, no restating the heading, no generic boilerplate; brevity on a routine
  flight is professional, not a defect.
* **Number-grounding (§3.3).** Grounds every claim in the provided figures
  (flight count / total time / distance / aircraft with units; area acreage); no
  vague quantities when an exact number exists.
* **Guard integrity (ADR-0029).** The number-grounding lever carries an explicit
  altitude carve-out — number-grounding does NOT extend to altitude, which stays
  neutral capture data; ranking/singling-out/tallying flights by altitude remains
  forbidden. The runtime detector `report_audience.py` is unchanged; new tests in
  `test_report_audience_guard.py::TestNarrativeQualityLevers` lock the levers and
  prove a report containing a 146.3 m AGL (480 ft) flight stays guard-clean. Full
  ADR-0029 / audience-leak suites pass unchanged (52 passed).
* **Caps unchanged** (ADR-0030). `.deployer-disabled` repo — hand-deploy on
  BOS-HQ; verify the public OpenAPI version (2.77.0), not `deployer-state.json`.

## 2026-07-03 — Advisory-lock the Alembic migration boot path (ADR-0036, Phase 1)

Migration-consolidation hardening, Phase 1 of
**docs/plans/2026-07-03-migration-consolidation.md**. Decision + rationale in
**docs/adr/0036-migration-single-path-hardening.md**.

* **Advisory lock on the migration run.** `run_migrations_sync()`
  (`backend/app/db_migrations.py`) now wraps its entire detect + stamp +
  upgrade critical section in a **session-level Postgres advisory lock**
  (`_MIGRATION_LOCK_ID = 8675310`) taken on a dedicated AUTOCOMMIT connection
  and released in a `finally`. Previously the migration path relied only on
  transaction atomicity + the `--workers 1` / single-replica assumptions — two
  backends booting concurrently (multi-worker, multi-replica, or a blue-green
  pair briefly pointing two backends at the same writable primary) could both
  enter `command.upgrade` and deadlock on a revision's DELETEs (0003) or
  double-apply DDL. The lock is **blocking** (`pg_advisory_lock`, not `try_`):
  a losing racer WAITS for the winner, then re-detects `current == head` and
  no-ops — it never skips the lock and proceeds against an un-migrated schema.
  Lock id is DISTINCT from `seed.py`'s `_SEED_LOCK_ID` (8675309) so migrating
  and seeding don't needlessly serialize against each other. Mirrors the
  posture the seed path already had. The ADR-0021 `pg_is_in_recovery()`
  primary-only guard is untouched.
* **Revision-id length invariant fence.** A hermetic test asserts every
  Alembic revision id is ≤ 32 chars (the `alembic_version.version_num`
  `VARCHAR(32)` that caused the v2.75.1 crash-loop when revision `0004` was
  41 chars, ran its DDL, then rolled back the stamp on every boot). CI now
  fails before such a revision can ship.
* **Tests.** `backend/tests/test_db_migrations.py` gains lock-envelope
  coverage: acquire-before-upgrade / release-after ordering, brownfield
  stamp+upgrade under lock, the no-op fast path still acquires+releases, the
  lock is released even when `command.upgrade` raises, and the lock id is
  distinct from the seed lock. Suite: 25 passed, 2 skipped (opt-in real-PG
  integration via `DOC_TEST_PG_URL`).
* **Scope.** Phase 1 only. `_add_missing_columns` / `_create_hot_indexes` and
  the legacy helpers in `main.py` are intentionally NOT removed here — the plan
  defers helper-severance (Phase 3) and the model-vs-head CI sync gate
  (Phase 2) to later, lower-urgency passes.
* **Deploy.** This repo is `.deployer-disabled` — the NOC deployer pulls git
  but does not rebuild. Ship via a hand-deploy on BOS-HQ
  (`docker compose build backend worker beat && up -d --no-deps …`); verify
  container build time, not `deployer-state.json`.

## 2026-07-03 — feat(missions): airspace / LAANC awareness at mission creation (ADR-0037)

Airspace/weather data was dashboard-only and not tied to mission creation.
Operators now get an **operator-facing pre-flight airspace check** at
scheduling time. Full design in **docs/adr/0037-airspace-laanc-awareness-at-mission-creation.md**.

* **New service `backend/app/services/airspace.py`.** `fetch_airspace_class()`
  point-in-polygon queries the FAA public Class Airspace ArcGIS FeatureServer
  (free, no key) — no intersecting polygon ⇒ uncontrolled Class G.
  `derive_laanc_requirement()` is tri-state: `True` for controlled B/C/D/
  E-surface, `False` for G, **`None` when undetermined** (never fabricate a
  safe-looking default from missing data). `assemble_preflight()` reuses the
  existing weather-router TFR/METAR/Open-Meteo fetchers and emits neutral
  advisories. `extract_latlon()` derives a coordinate from a mission's
  free-form `area_coordinates` (flat/aliases, `center`, GeoJSON Point/Polygon).
* **New endpoints (`backend/app/routers/missions.py`).**
  `GET /api/missions/airspace-preflight?lat=&lon=&airport=` (primary) and
  `GET /api/missions/{mission_id}/preflight`. Returns `{airspace_class,
  laanc_likely_required, controlling_facility, tfrs, weather, advisories,
  degraded, disclaimer}`. Static preflight route is declared before
  `/{mission_id}` so the path isn't captured as a mission id.
* **Computed on demand, never persisted.** TFRs/weather are time-varying; a
  create-time snapshot would be stale by flight day. No schema change, no
  migration → failover-safe. The `create_mission` write path is unchanged.
* **Graceful degradation.** All feeds gathered with `return_exceptions=True`;
  any feed failing (or raising) yields partial data + `degraded: true` +
  advisory — **never a 500**. Undetermined airspace ⇒ `laanc_likely_required:
  null`.
* **Operator-facing ONLY — never in the client report (ADR-0029 boundary).**
  Preflight is never persisted on the mission, never passed to any report
  builder, and renders no compliance verdict. Guarded by
  `tests/test_report_never_references_airspace.py` (fails if any report module
  or the mission schema references airspace/laanc/preflight/tfr) and by a unit
  test asserting no advisory contains "violation/illegal/non-compliant".
* **Tests.** +44 (`tests/services/test_airspace_service.py`,
  `tests/test_missions_airspace_preflight.py`,
  `tests/test_report_never_references_airspace.py`). Suite 507 → 551 passing,
  0 regressions.

## 2026-07-03 — fix(reports): resolve the aircraft from the live flight, not the stale junction copy (ADR-0038) — v2.78.0

Phase 1 of the flight-attach unification (plan:
`docs/plans/2026-07-03-flight-attach-unification.md`) — the **root fix** for the
ADR-0033 junction-staleness class.

* **The bug.** The `MissionFlight` junction copied `Flight.aircraft_id` at attach
  time. When a fleet serial was registered **later**, the live flight updated but
  the junction copy stayed stale — the Avata 2 mechanism. ADR-0033 made the
  report *tolerant* of a NULL copy (fell back to parsed `drone_model`), but never
  read the live fleet record, so a late-linked flight showed the bare string
  `"Avata2"` instead of the canonical `"DJI Avata 2"` card (name/image/specs).
* **Read convergence** (`backend/app/routers/reports.py`). `_load_live_flight_metrics`
  now LEFT-JOINs the fleet `Aircraft` (scalar columns only — the heavy Flight
  JSON is still never loaded, ADR-0025/0019). `_aircraft_label` and the new
  `_build_aircraft_cards` (extracted from the PDF path) resolve **native** flights
  from the live `Flight.aircraft`; **legacy-ODL** (`flight_id IS NULL`) rows keep
  the junction/cache read (Phase 2 materializes them). One resolver drives both
  the narrative label and the PDF "Aircraft used" card.
* **Write change** (`backend/app/routers/missions.py`). The single-add and bulk
  native attach paths no longer copy `aircraft_id` onto the junction (set NULL —
  derived on read; client-sent values still ignored, ADR-0007). The column is
  **retained** (drop is Phase 4).
* **Behaviour.** Preserving for reports **except** the fix: a native flight linked
  after attach now shows the correct fleet aircraft with no detach/re-attach.
* **Defers (per plan):** legacy-ODL materialization (Phase 2); metrics/track
  live-only flip + zero-cache-read counter (Phase 3); column drops + `flight_id`
  NOT NULL (Phase 4). ADR-0007 matcher and ADR-0029 audience guard untouched.
* **Tests.** New `backend/tests/test_report_live_aircraft_adr0038.py`: real-DB
  late-link root-fix proof (junction stays NULL, live resolves), legacy-ODL
  no-regression, unlinked-native `drone_model` fallback, stale-copy-ignored.
  Fail-before/pass-after confirmed (`"Avata2"` → `"DJI Avata 2"`). Full backend
  suite green (511 passed, 3 skipped). Existing attach-derives-aircraft tests
  updated to the new "junction not copied" contract.
* **Deploy.** `.deployer-disabled` — manual BOS-HQ rebuild
  (`docker compose build backend worker beat flight-parser && up -d --no-deps …`);
  verify the public `openapi.json` version (`2.78.0`), not `deployer-state.json`.

## 2026-07-03 — Avata 2 report incident: data remediation + prod deploy (ADR-0033)

Follows the code fix below. Full incident write-up + audit trail in
**docs/adr/0033-avata2-missing-from-report-incident.md**.

* **Data remediation (prod).** Root cause was a blank `serial_number` on the
  fleet `DJI Avata 2` record, so ADR-0007's strict serial-first matcher left
  every recent Avata flight unlinked. Registered serial `1581F6W8A242N0A3` and
  backfilled: aircraft 1 row, `flights` 12 rows (unlinked 12 → 0),
  `mission_flights` 4 rows. The "Springfield Drifters Promo" mission now resolves
  `DJI Avata 2 ×2 + DJI Mini 5 Pro ×1`; regenerating the report renders the Avata
  correctly. **Standing rule:** register a drone's serial when adding it to the
  fleet, or its flights stay unlinked under ADR-0007.
* **Deploy.** This repo is `.deployer-disabled` — the NOC deployer pulls git but
  does not rebuild. The reports fix + the ADR-0032 parser fix were hand-deployed
  on BOS-HQ (`docker compose build backend worker beat flight-parser && up -d
  --no-deps …`). Verify container build time, not `deployer-state.json`, to
  confirm a DOC deploy is actually running.

## 2026-07-03 — fix(reports): attached flight with unrecognized aircraft no longer missing from report

An attached flight whose fleet aircraft was unrecognized (`flights.aircraft_id`
NULL) was dropped from — or genericized to "Unknown" in — the client report.

* **Field defect.** The 2026-07-02 "Springfield Drifters Promo" mission had a DJI
  Avata 2 flight attached (native `flight_id`), but the generated report omitted
  it. Root cause: the Avata flight carried a `drone_serial` that the fleet
  "DJI Avata 2" aircraft record lacked (its `serial_number` is blank), so the
  strict serial-match path (ADR-0007) refused a model fallback and left
  `aircraft_id` NULL. `backend/app/routers/reports.py` then read ONLY
  `MissionFlight.aircraft` and substituted the literal "Unknown" — discarding the
  flight's own parsed `drone_model` ("Avata2"). The flight was attached the whole
  time; the report layer was not robust to an unlinked aircraft.
* **Fix (report layer, defense-in-depth).** New `_aircraft_label()` resolves the
  aircraft display name with a fallback chain: linked fleet `model_name` → live
  `Flight.drone_name`/`drone_model` → cache `drone_name`/`drone_model`/`aircraft`
  → "Unknown" only as a true last resort. `_build_flight_summaries` (the LLM
  aggregation) and the PDF "Aircraft used" section both use it, so an attached
  flight is never silently dropped from either surface. `_load_live_flight_metrics`
  now also selects `drone_model`/`drone_name` (scalar columns; heavy JSON still
  never loaded, ADR-0019). Regression test:
  `backend/tests/test_report_unrecognized_aircraft_label.py`.
* **No compliance logic touched** — the ADR-0029 altitude/Part-107 exceedance
  prohibition stays intact.
* **Operator residual (data, not code).** To restore canonical fleet attribution
  (and the aircraft image/specs card), add serial `1581F6W8A242N0A3` to the
  "DJI Avata 2" fleet aircraft record, then POST `/api/flights/backfill-aircraft`.
  Until then the report labels the flight "Avata2" from the parsed model.

## 2026-07-02 — fix(flight-parser): correct DJI voltage, Litchi/Airdata speed units, Airdata altitude selection

Three confirmed flight-log parser correctness bugs that put wrong numbers into
client-facing report data. All fixed with new per-format tests (the Litchi and
Airdata parsers previously had zero test coverage). Candidate for a new ADR
(parser unit-correctness; number TBD by the operator).

* **`flight-parser/src/dji.rs` — DJI battery voltage was 1000× too small.** The
  frame loop did `battery.voltage as f64 / 1000.0`, but `dji-log-parser` 0.5.7
  already returns `FrameBattery.voltage` in **volts** (its `SmartBattery` and
  `CenterBattery` record parsers map the raw `u16` with `/1000.0` — confirmed in
  the crate's `src/record/smart_battery.rs` and `src/record/center_battery.rs`).
  The extra divide turned a 15.2 V pack into 0.0152 V and disagreed with the
  Airdata parser (which stores volts raw). Extracted a tested
  `frame_battery_voltage()` normaliser that passes volts through unchanged.

* **`flight-parser/src/litchi.rs` — Litchi speed was stored without unit
  conversion.** Litchi CSVs export `speed(mph)` (some km/h); the value was
  summed into `max_speed` and every track speed with no conversion, inflating
  speed by ~2.237× (mph) / ~3.6× (km/h). Now detects the unit from the matched
  header and normalises to m/s, mirroring the Airdata parser. Also fixed the
  time-column selection: the old `contains("time")` could bind the numeric
  epoch-ms `timestamp` column instead of `datetime(utc)`, collapsing duration to
  the point-count fallback — now prefers an explicit `datetime` column and never
  binds `timestamp`.

* **`flight-parser/src/airdata.rs` — metric Airdata speed + altitude
  selection.** Added a km/h → m/s branch (metric exports were treated as m/s,
  ~3.6× inflated). Lowercased the dead `altitude_above_seaLevel(feet)` candidate
  (its capital `L` never matched a lowercased header) and demoted sea-level (MSL)
  below the AGL / relative-altitude candidates, adding an explicit
  `height_above_takeoff(m)` entry — so a metric export exposing both
  `height_above_takeoff(m)` and `altitude_above_sealevel(m)` now reports the AGL
  value for `max_altitude`, not the (much larger) MSL value.

No altitude/Part-107 exceedance flagging was added — these are unit-correctness
fixes only (ADR-0029: reports are client deliverables, not compliance audits).

Verification: `cargo test` in a `rust:1-slim` container — 20/20 pass (14
pre-existing + 6 new); clean `cargo build`, zero warnings.

## 2026-07-01 — fix(reports): remove disproven "unverified peak" ODL altitude caveat — v2.76.3

ODL-imported flights at the ~500 m DJI device ceiling were tagged in client
reports as `" — unverified (device-reported maximum, not a measured peak)"`. That
caveat was a defensive residue from ADR-0028 H1, never validated. It is **false**
and is removed. See **ADR-0031**.

**Ground-truth verification** (author bill-bg, 2026-07-01): device-ceiling
flights' max-altitude readings are self-consistent on a per-aircraft basis and
agree with post-flight telemetry reviews. The warning was a false overprotection.
Removed the caveat from the report narrative template; the underlying metric
(max altitude in feet/meters from the parsed flight log) stands.
