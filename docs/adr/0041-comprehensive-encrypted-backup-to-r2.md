# ADR-0041 — Comprehensive encrypted backup of all DroneOpsCommand state to Cloudflare R2

- **Date:** 2026-08-17
- **Status:** **Accepted** — implemented, deployed and verified end-to-end on
  BOS-HQ 2026-08-17. As-built deltas and the residual operator action are
  recorded in "Implementation outcome" at the foot of this document.
- **Supersedes (operationally):** the plaintext `aws s3 cp` / `s3 sync` legs of
  `scripts/snapshot.sh` (CHANGELOG 2026-07-16). Keeps and extends the freshness
  metric + quarterly restore drill (CHANGELOG 2026-07-22).
- **Related:** ADR-0012 (secret hygiene), ADR-0018 (deploy path), fleet ADR-0036
  (ntfy transport), fleet ADR-0037 (notification noise policy), fleet ADR-0086
  (1Password Fleet vault filing).

---

## Context

The brief that triggered this work assumed DroneOpsCommand had **no** live
backup. That premise is **false** and is corrected here, because the correction
changes the whole shape of the decision.

**What is actually running (verified on BOS-HQ, 2026-08-17):**

| Fact | Evidence |
|---|---|
| Nightly backup runs | `crontab -l` → `23 3 * * * ~/droneops/scripts/snapshot.sh` |
| Last successful run | `snapshot.log` → `2026-08-17T03:23:02Z … done. db=55M … uploads=synced` |
| Off-host copy exists | `s3://obs-glitchtip-backups/droneops/` — 229 objects, 2.3 GiB |
| Freshness metric live | `/var/lib/node_exporter/textfile_collector/droneops_backup.prom` = `1786937015` |
| Stale-backup alert live | Grafana `obs-rule-droneops-backup-stale`, `> 28h` or `absent()*9999`, `severity: high`, not paused |
| Restore drill installed | `droneops-restore-drill.timer` (quarterly, 16th Jan/Apr/Jul/Oct 16:23 UTC) |
| Restore drill has succeeded | `droneops_restore_drill.prom` = `1784698698` → 2026-07-22 |

So the backup lane is real, monitored, and has been proven restorable once. This
ADR is **not** "build backups". It is "close the seven gaps that remain", and
the most serious of those are not about coverage — they are about *encryption*,
*history*, and *off-host retention*.

### Gap 1 — Backups are stored in plaintext (severity: high)

`snapshot.sh` writes `pg_dump | gzip` straight to R2 with `aws s3 cp`, and
mirrors `uploads/` with `aws s3 sync`. Nothing is encrypted by us; we rely
solely on Cloudflare's at-rest encryption, with a key Cloudflare holds.

The payload is not low-sensitivity:

- `customers` (7 rows) — names, contact details, billing addresses
- `line_items`, invoice/payment state — financial records
- `tos_acceptances` (10 rows) + `uploads/tos_signed/` (11 signed PDFs) —
  **executed legal documents with client signatures**
- `device_api_keys` (6 rows) — credential material for field controllers
- `flights` (780 rows, 97 MB) — customer site telemetry and imagery references

ADR-0012 established that this project does not tolerate secrets in
low-control locations. A plaintext dump of all of the above, in a bucket
shared with three other services, is inconsistent with that stance. The fleet
already made the opposite call for equivalent data: DR3-Vision encrypts its
dump with restic *specifically* because it carries payroll/PII.

### Gap 2 — The R2 credential is over-scoped (severity: high)

`snapshot.sh` sources `/opt/observability/.env` and reuses
`OBS_GLITCHTIP_BACKUPS_R2_*`. The bucket `obs-glitchtip-backups` also holds
`callsign/`, `claudesync/` and `2026/` prefixes. Consequences:

- The DroneOps backup job holds read/write on **CallSign and ClaudeSync
  backups** it has no business touching.
- Conversely, a compromise or rotation of the glitchtip observability
  credential silently breaks or exposes DroneOps DR.
- Blast radius of a single leaked key is four services, not one.

### Gap 3 — `uploads/` has no history, only a mirror (severity: high)

`aws s3 sync` without `--delete` is an *additive mirror*. It protects against
volume loss. It does **not** protect against the more likely failure: a file
overwritten with corrupt content, or truncated by a bad ingest. The next sync
faithfully copies the damage over the only remaining good copy. There is no
snapshot to roll back to. This covers 644 MB / 196 objects, including
`uploads/flight_logs/` (170 raw DJI flight records, 640 MB) and the signed
legal PDFs.

### Gap 4 — Nothing prunes the R2 side (severity: medium, cost + compliance)

The retention sweep in `snapshot.sh` is
`find "${BACKUP_DIR}" -mtime +14 -delete` — it prunes the **local** copy only.
R2 currently holds 33 daily dumps (1.6 GiB) going back to install day
(2026-07-16) and will grow without bound at ~54 MB/day (≈20 GB/yr). There is
also no defined retention *policy* — dumps of executed legal documents are
being kept forever by accident rather than by decision.

### Gap 5 — Host configuration is not backed up at all (severity: **critical**)

Neither `~/droneops/.env` (mode 600, 41 keys) nor
`~/droneops/docker-compose.override.yml` (mode 600, holds the inlined promoted
`DATABASE_URL`) is captured anywhere. They contain `POSTGRES_PASSWORD`,
`REPLICATION_PASSWORD`, `JWT_SECRET_KEY`, `CLOUDFLARE_TUNNEL_TOKEN`,
`SMTP_PASSWORD`, `NTFY_DRONEOPS_PUBLISHER_TOKEN`, `SENTRY_DSN`.

**This is the gap that makes the current backup incomplete as a DR artifact.**
Restore the database and the uploads onto a fresh host today and the stack
still does not come up: compose refuses to start (`:?` guards on
`DATABASE_URL`, `JWT_SECRET_KEY`, `POSTGRES_PASSWORD`, `REPLICATION_PASSWORD`
per ADR-0012), the tunnel has no token, and every issued JWT is invalidated
because the signing key is gone. Recovery would be a manual re-derivation of
41 values from memory and 1Password.

It is also the second independent argument for encryption: a `.env` of this
kind *cannot* be placed in a bucket unencrypted, so Gap 5 and Gap 1 must be
closed by the same mechanism.

### Gap 6 — `reports/` is not backed up (severity: low)

`snapshot.sh` syncs `/data/uploads` only. `/data/reports` (13.2 MB, 30
rendered `map_*.png` and report artifacts) is skipped. These back the 7 rows in
`reports` — client deliverables under ADR-0029. Nominally regenerable, but
regeneration depends on the Ollama narrative path and external map tiles and is
not bit-reproducible. 13 MB is not worth arguing about; include it.

### Gap 7 — WAL archiving is configured, consuming disk, and dead (severity: medium)

This one is worth stating precisely, because it looks like PITR and is not.

```
archive_mode    = on
archive_command = cp %p /var/lib/postgresql/data/wal_archive/%f
archive_timeout = 0
```

- The archive destination is **inside the pgdata volume it is meant to
  protect** (`droneops_standby_pgdata`). It survives no failure that the volume
  does not survive. It has never been pushed off-host.
- It has never been pruned: **352 segments, 5.5 GiB**, oldest `2026-04-20`. That
  is 68% of the 8.0 GiB volume — the database itself is 109 MB.
- `archive_timeout = 0` means a segment is only archived when it fills (16 MiB).
  Write volume is so low that `pg_stat_archiver.last_archived_time` is
  **2026-08-01 16:15** — as of 2026-08-17 there is a **16-day hole** in the
  archive. Even on the same volume, it could not perform a PITR to any recent
  point.

So PITR is not a capability we currently have; it is a 5.5 GiB liability
wearing PITR's clothes. Any decision about PITR has to start from that.

### What is *not* broken (preserve it)

- **Fail-closed error handling.** `snapshot.sh` calls `fail()` and exits
  non-zero on every error path. It does **not** copy DR3-Vision's fail-soft
  `exit 0`-when-env-missing behaviour, which would silently produce zero
  backups while the timer looked healthy. Keep the fail-closed posture.
- **Death detection.** The freshness metric plus the Grafana rule's
  `absent(...) * 9999` clause is exactly the defence the old HSH-HQ path lacked
  when it stopped on 2026-04-15 and nobody noticed for four months. An unset
  gauge would otherwise read as a permanently-green dead man.
- **Empty/truncated-dump guard.** `[[ ! -s "$OUT" ]] || ! gzip -t` catches a
  dump that "succeeded" into nothing.
- **The quarterly restore drill**, including its `MIN_FLIGHTS_RATIO_PCT=90`
  check — a near-empty restore that exits 0 is the failure this drill exists to
  catch.

---

## Decision

Migrate the DroneOpsCommand backup lane to **restic → a dedicated Cloudflare R2
bucket**, expand coverage to every stateful artifact required for a
from-nothing rebuild, enforce retention off-host, and retire the dead WAL
archive. Keep the existing freshness-metric and restore-drill machinery intact.

### D1 — restic replaces `aws s3 cp` / `s3 sync`

Repository: `s3:<R2_ENDPOINT>/droneops-backups/restic`, restic AES-256 with a
`RESTIC_PASSWORD` held in 1Password. Runner: the pinned container image
`restic/restic:0.17.3` (already present on BOS-HQ), matching the DR3-Vision
pattern, rather than the host's `restic 0.16.4` — so the tool version is
declared in the repo and cannot drift with an apt upgrade.

Four reasons, in order of weight:

1. **Encryption** closes Gap 1 and is a hard prerequisite for Gap 5.
2. **Snapshots** close Gap 3 — `uploads/` gains real per-day history instead of
   a mirror that propagates corruption.
3. **`forget --prune`** closes Gap 4 by enforcing retention *in R2*, which
   `aws s3 cp` structurally cannot do.
4. **`restic check --read-data-subset`** gives continuous proof the repository
   is readable, between quarterly drills.

**Dedup is the reason this costs nothing, and it hinges on one detail:** the
dump must be piped into restic **uncompressed**. Today's `pg_dump | gzip -9`
produces a completely different byte stream every night (53.2 MB → 56.7 MB over
14 days, but essentially zero shared blocks), so a naive restic-over-gzip would
store a fresh ~55 MB every night and gain nothing. Feeding restic the plain
dump lets content-defined chunking dedupe the ~99% of the corpus that does not
change, and restic's own zstd compression recovers the space. This is the
single most important implementation detail in this ADR.

### D2 — Dedicated bucket, dedicated credential

New R2 bucket `droneops-backups` with its own bucket-scoped API token, closing
Gap 2. Secret material lives in `~/.droneops-secrets/restic-droneops.env`
(mode 600) on BOS-HQ, and is filed to the 1Password Fleet vault per ADR-0086.

`RESTIC_PASSWORD` is the at-rest key: **if it is lost, every backup is
permanently unreadable.** The on-host copy sits on the machine being backed up
and is therefore worthless in the disaster it exists for. The 1Password copy is
the one that matters.

### D3 — Four backup lanes (complete DR coverage)

| Lane | Source | Size now | Rationale |
|---|---|---|---|
| `db` | `pg_dump -Fc` of `droneops` on `droneops-standby-db`, via `--stdin` | 109 MB logical | Primary state. `-Fc` for selective `pg_restore`. |
| `files` | `droneops_app_data` → `/data/uploads` + `/data/reports` | 657 MB | Flight records, signed legal PDFs, deliverables (Gaps 3, 6) |
| `config` | `~/droneops/.env`, `docker-compose.override.yml`, `.env.example`, `docker-compose*.yml`, `crontab -l` | < 100 KB | **Closes Gap 5.** Encryption mandatory. |
| `legacy` | one-shot: last pre-migration dump `~/backups/postgres/droneops_20260415_020001.dump` on droneops-server | 26 MB | Preserves the final HSH-HQ state before the 13 GB zombie tree is deleted |

**Explicitly excluded, with reasons recorded so nobody re-litigates them:**

- `droneops_ollama_data` (4.6 GB) — `llama3.1:8b-instruct-q4_K_M` weights,
  re-pullable from the Ollama registry by `ollama-setup`. Backing this up would
  multiply repository size by 7 to protect a public artifact.
- `droneops_postgres_data` (46 MB) — the neutralized pre-2026-04-20 primary.
  Superseded by `droneops_standby_pgdata`; retained on disk, not worth a lane.
- `/data/backups` (68.6 MB) — two stale in-app `doc_backup_*.dump` files
  (2026-05-04, 2026-06-02). Stale copies of data the `db` lane captures
  properly. **Delete**, do not archive.
- `droneops-gw_caddy_data` (24 KB) — ACME state, self-regenerating.
- `droneops_standby_pgdata` as a *filesystem* — a physical copy of a running
  cluster is not a valid backup; the logical dump is the correct artifact.
- Redis (`droneops-redis-1`) — Celery broker, ephemeral by design.
- The `droneops-demo` stack (48 MB db, 9.4 MB app data) — non-production
  demonstration data, reseedable.

### D4 — Retention: 14 daily / 8 weekly / 24 monthly / 7 yearly

Deliberately longer than DR3-Vision's 7d/4w/12m/5y:

- **14 daily** preserves the 14-day window operators already have locally, so
  the migration does not quietly shorten recovery reach.
- **8 weekly** covers a two-month regression window — long enough to catch a
  slow data-integrity defect of the ADR-0028 class.
- **24 monthly / 7 yearly** is driven by content, not by database convention:
  this repository holds executed TOS documents and invoice records. Seven years
  is the ordinary retention floor for business financial records.

Affordable precisely because of D1's dedup — see Consequences.

**Amended 2026-08-18 (operator decision):** yearly retention raised from 7 to
**unlimited** — Bill: "retention is indefinite." Yearly snapshots are never
pruned. Applies to backup snapshot history only; live production data was
never subject to any retention. Marginal cost is a few deduplicated MB per
year. Shipped as `KEEP_YEARLY=unlimited` in `scripts/droneops-backup.sh`
(restic `forget --keep-yearly unlimited`, supported natively by the pinned
0.17.3).

### D5 — Retire WAL archiving; do not adopt PITR

Set `archive_mode = off`, delete `wal_archive/` (reclaims 5.5 GiB), and accept a
**24 h → 12 h RPO from nightly/twice-daily logical dumps**.

Reasoning, from measurements rather than instinct:

- Write volume does not justify PITR. One 16 MiB WAL segment has been produced
  in the last 16 days. Real change is on the order of ~1 MB/day; the
  compressed dump grew 3.5 MB across 14 days.
- The blast radius of a 24 h RPO is small and *recoverable by other means*.
  780 flights over ~4 months is ≈6.5 flights/day, and every flight record has a
  re-uploadable source on the controller (ADR-0002 / DroneOpsSync ingest path).
  A day's loss is a re-upload, not a permanent loss.
- Doing PITR *properly* means `pg_receivewal` streaming to a separate volume
  and into restic. Doing it the naive way (`archive_timeout=300`) would write
  ~4.6 GB/day of mostly-empty padded segments to protect ~1 MB/day of change.
  Neither is warranted at this write volume.
- The status quo is strictly worse than either: it costs 5.5 GiB and delivers
  nothing.

**Mitigation instead of PITR:** move to **twice-daily dumps (03:23 and 15:23
UTC)**, halving RPO to 12 h at effectively zero marginal cost, since restic
dedups the second run of the day to near nothing.

**Escape hatch, recorded now so it is not rediscovered later:** if RPO ever
needs to be minutes (e.g. multi-tenant write volume grows an order of
magnitude), the correct path is `pg_receivewal` → dedicated volume → a fifth
restic lane. It is *not* re-enabling `archive_command` into pgdata.

`archive_mode` requires a **restart**, not a reload. The `chad_hq_standby`
physical replication slot is `active`/`reserved` and streams via walsender —
independent of `archive_command` — so this is safe, but the implementer must
confirm the CHAD-HQ standby has no `restore_command` pointing anywhere before
flipping it.

### D6 — systemd timers replace cron; keep UTC

Replace the crontab line with `droneops-backup.{service,timer}`, installed as
**system** units with `User=bbarnard065` — matching the sibling
`droneops-restore-drill.service` already in `/etc/systemd/system/` and in
`scripts/systemd/`. Benefits over cron: `Persistent=true` catches missed runs
after a reboot, `journalctl -u` gives real run history, and `RandomizedDelaySec`
de-synchronizes from the other nightly jobs.

**Keep `OnCalendar` in UTC**, not `America/Los_Angeles`. The BOS-HQ nightly
window is stacked in UTC (docs 02:44, droneops 03:23, callsign 03:30,
glitchtip 03:32, claudesync 03:46) and a DST-shifting local-time entry would
collide with a sibling job twice a year. DR3-Vision uses local time because it
runs on svdp-dev; the constraint here is different. *(Operator note per fleet
convention: 03:23 UTC = 20:23 PDT the previous evening.)*

### D7 — Alerting stays on `infrawatch-alerts`

The brief proposed a new `droneops-backup` topic. **Rejected**, in favour of the
existing routing, on the strength of the 2026-07-14 operator "no new topics"
decision already encoded in `snapshot.sh`. A new topic means minting a
publisher token, a service-registry row, a `ntfy-fallback-topics.yml` entry, and
a new phone subscription — for an alert class that is already delivered and
already proven. Surface it to Bill; do not change it unilaterally.

Grading against the ADR-0037 five-question gate:

| # | Question | Verdict |
|---|---|---|
| 1 | Actionable within 5 min? | Yes — re-run the script by hand, or check R2 credentials. |
| 2 | Customer-visible? | No — hence `high`, never `urgent`. Imminent-data-loss *risk*, not impact. |
| 3 | Self-healed first? | Yes — the Grafana rule fires on **staleness > 28 h**, i.e. after a full missed cycle, not on one transient failure. |
| 4 | Deduplicated? | Yes — `--dedup-key droneops-backup --cooldown 21600` (6 h), verified supported by `~/.local/bin/ntfy-publish.sh`. |
| 5 | Useful click target? | **No — this is a defect.** The current `fail()` sends no `Click`. Fix: add `--click https://noc-mastercontrol.barnardhq.com/status/droneops` (tier-3 per ADR-0036; note `noc-mastercontrol`, not `noc`). |

Passes at `high` once #5 is fixed.

### D8 — Verification is continuous, not just quarterly

- **Every run:** `restic check` (metadata/structure integrity).
- **Weekly:** `restic check --read-data-subset=5%` — actually reads and verifies
  pack data, catching bit-rot the metadata check cannot see. Matches the Docs
  Hub precedent.
- **Quarterly:** the existing `restore-drill.sh`, **adapted to restic** — it
  currently pulls the newest `.sql.gz` via `aws s3 cp`. Its 90%-of-live-flights
  ratio assertion must be preserved verbatim.

**Hard constraint for the implementer:** the metric names
`droneops_backup_last_success_timestamp_seconds` and
`droneops_restore_drill_last_success_timestamp_seconds` are consumed by
`obs-rule-droneops-backup-stale` and `obs-rule-droneops-restore-drill-stale` in
`/opt/infrawatch/grafana/provisioning/alerting/observability-alerts.yml`.
Renaming either without editing that file in the same change converts a live
alert into a permanently-green dead man — the exact failure this lane was built
to prevent.

### D9 — Retire the droneops-server zombie path explicitly

`~/backups/postgres/droneops_*.dump` on droneops-server stops at 2026-04-15;
`~/backups/wal-archive/droneops` is empty; no cron entry or timer references
either. It is dead and correct to be dead — droneops moved to BOS-HQ on
2026-04-20.

After the `legacy` lane (D3) captures the final 2026-04-15 dump, delete
**only** `droneops_*.dump` and the empty `wal-archive/droneops` directory.

**Do not delete `~/backups/` wholesale.** It also contains ~840 MB of
`n8n_*.sqlite` files owned by `root` and out of scope for this ADR. Removing
them requires a separate decision by their owner.

---

## Options considered

### Option A — Leave `snapshot.sh` as-is, add only the missing `.env` lane

- **Pros:** minimal change; the existing lane is working and proven once.
- **Cons:** cannot be done. Putting a plaintext `.env` containing
  `JWT_SECRET_KEY`, `POSTGRES_PASSWORD` and `CLOUDFLARE_TUNNEL_TOKEN` into a
  bucket shared with three other services directly contradicts ADR-0012.
  Leaves Gaps 1–4 open.
- **Rejected:** the critical gap cannot be closed without also closing the
  encryption gap.

### Option B — Keep `aws-cli`, add client-side `age`/`gpg` encryption

- **Pros:** smallest diff from the current script; keeps `aws s3 cp` ergonomics.
- **Cons:** encryption alone fixes Gap 1 and nothing else. Still no snapshot
  history for `uploads/` (Gap 3), still no R2-side retention (Gap 4), still no
  integrity verification, and it makes dedup strictly worse — every encrypted
  dump is a fresh opaque blob, so storage grows ~55 MB/day forever. Also
  introduces a *second* independent key to lose.
- **Rejected:** more moving parts than restic, fewer capabilities.

### Option C — restic (chosen)

- **Pros:** encryption, snapshot history, off-host retention enforcement, and
  integrity verification in one tool. Strong fleet precedent — DR3-Vision,
  Docs Hub, SVDP-Site, helix-hub all run restic → R2, so the operator already
  knows the restore idiom. Dedup makes 7-year retention essentially free.
- **Cons:** loses the "download a `.sql.gz` from the R2 dashboard" escape
  hatch — restore now *requires* restic and the password. `RESTIC_PASSWORD`
  becomes a single point of catastrophic failure.
- **Mitigations:** password filed in 1Password Fleet vault (ADR-0086); the
  local 14-day plain `.sql.gz` retention on BOS-HQ is **kept** as the
  break-glass path that needs no key at all; the runbook documents the full
  restore end to end.

### Option D — Full PITR via `pg_receivewal` → restic

- **Pros:** RPO in seconds rather than hours.
- **Cons:** a fifth lane, a long-running streaming process to monitor (a silent
  `pg_receivewal` death is a new dead-man class), and materially more
  operational surface — to protect a database producing ~1 MB of change per day
  whose worst-case loss is re-uploadable from the controllers.
- **Rejected for now, documented as the escape hatch in D5.**

---

## Consequences

### Positive

- **Every artifact needed for a from-nothing rebuild is captured**, including
  the host configuration that currently blocks recovery entirely.
- **All backups encrypted at rest with a key we hold**, satisfying ADR-0012 for
  the signed legal documents and customer PII.
- **`uploads/` gains real history** — a corrupted flight log becomes
  recoverable instead of silently overwritten.
- **~5.5 GiB reclaimed on BOS-HQ** from the dead WAL archive, plus ~68 MB from
  stale in-app dumps and ~26 MB (post-archival) on droneops-server. R2 storage
  drops from a 20 GB/yr trajectory to a few GB total.
- **Credential blast radius shrinks** from four services to one.
- **RPO improves** 24 h → 12 h via twice-daily dumps.

### Negative / accepted risks

- **`RESTIC_PASSWORD` loss = total, unrecoverable backup loss.** Mitigated by
  1Password filing and the retained local plain dumps.
- **Restore is slower and less obvious than `aws s3 cp`** — mitigated by the
  step-by-step runbook and the quarterly drill that exercises it.
- **No PITR.** Accepted; sized against actual write volume, with a documented
  path to add it.
- **A migration window exists.** The new lane must be proven green *before* the
  cron line and the old R2 prefix are removed. Until then both run — cheap
  insurance, and the correct order.

### Cost

Cloudflare R2: $0.015/GB-month storage, $4.50/M Class A ops, **zero egress**.

| Lane | Year-1 estimate | Basis |
|---|---|---|
| `db` | ~0.6 GB | ~80 MB deduped base + ~10 MB unique per retained snapshot |
| `files` | ~1.5 GB | 657 MB now; flight logs are immutable so dedup is near-total |
| `config` + `legacy` | ~0.05 GB | |
| **Total** | **~2.2 GB → ~$0.03/month** | |

Class A operations: a restic run writes on the order of 50–200 pack objects;
at twice daily that is ≈10k ops/month ≈ **$0.05/month**.

**All-in: well under $1/year — and materially cheaper than the current
un-pruned ~20 GB/yr trajectory.** Storage cost is not a design constraint here,
which is exactly why 7-year yearly retention is the affordable choice rather
than an extravagant one.

---

## Verification (definition of done)

1. `restic snapshots` shows one snapshot per lane, tagged, dated today.
2. `restic check --read-data-subset=5%` exits 0.
3. A **restore rehearsal on a throwaway DB** proves the dump restores and
   `flights` row count is ≥90% of live — not merely that the snapshot exists.
4. `.env` restored from the `config` lane byte-matches the live file
   (`sha256sum` comparison).
5. The freshness metric advances after the first timer-driven run, and
   `obs-rule-droneops-backup-stale` is confirmed **not firing**.
6. A **deliberately failed run** (temporarily bad R2 credential) produces an
   ntfy `high` on `infrawatch-alerts` **with a working click URL**, and the
   script exits non-zero. A backup alert path that has never been seen to fire
   is a hypothesis, not an alert.
7. `wal_archive/` is gone, `archive_mode` reads `off`, and the
   `chad_hq_standby` slot is still `active` afterwards.
8. Only after 1–7 pass: the crontab line is removed and the old
   `droneops/db/` + `droneops/uploads/` R2 prefixes are deleted.

---

## Implementation outcome (2026-08-17)

Implemented, deployed to BOS-HQ, and verified against a 12-point adversarial
matrix. Everything in **Decision** shipped as written except where noted below.

### Verified

| # | Check | Observed |
|---|---|---|
| V1 | Snapshots per lane | 4 lanes present: `db`, `files`, `config`, `legacy` |
| V2 | `check --read-data-subset=5%` | exit 0, "no errors were found" |
| V3 | **Dedup proof** | 2nd full run added **414 KiB** (`db` lane: 461 MB processed → 1.813 MiB added; `files` lane: 656.8 MB → **0 B**). Raw-data 711.863 → 712.267 MiB. |
| V4 | DB restore rehearsal | `flights` **780/780** (100%, floor 90%), `battery_logs` 779, `tos_acceptances` 10 |
| V5 | Config restore proof | restored `.env` sha256 `8839dd0d1736ddf3…` == live |
| V6 | Files restore proof | signed TOS PDF, flight log and a `reports/` PNG all byte-identical |
| V7 | Failure path | corrupt R2 secret → exit **1**, ntfy `high` on `infrawatch-alerts`, title `[DroneOps Command] nightly backup FAILED`, click URL resolves **HTTP 200** |
| V8 | Cooldown | two failures inside 6 h → **exactly one** notification |
| V9 | Freshness metric | present in Prometheus, staleness 0.09 h, rule **not firing**, `absent()` empty |
| V10 | Grafana contract | alert YAML, both scripts and Prometheus all agree on the two metric names; YAML untouched |
| V11 | WAL retirement | `archive_mode=off`, `wal_archive` gone, volume **8.0G → 2.5G**, `chad_hq_standby` slot still `active`/`reserved`, standby caught up (`sent_lsn == replay_lsn`) |
| V12 | systemd | `systemctl start droneops-backup.service` → `Result=success`, `ExecMainStatus=0` |

### Deltas from the plan as drafted

1. **The dedicated bucket and a bucket-scoped token were provisioned in this
   change, so Gap 2 is closed now rather than deferred.** The plan assumed the
   operator would mint these by hand. Bucket `droneops-backups` (WNAM) was
   created via the Cloudflare API, and a token scoped to
   `…r2.bucket.<acct>_default_droneops-backups` with only *Bucket Item
   Read + Write* was minted for it. The backup lane no longer touches the
   account-wide `OBS_GLITCHTIP_BACKUPS_R2_*` credential; blast radius drops
   from four services to one.

2. **No per-run date tag.** D3's lane table is unchanged, but the drafted plan
   also suggested tagging each snapshot with `$(date -u +%F)`. That is
   **incompatible with `--group-by tags`**: restic groups on the *full* tag
   set, so every snapshot would form its own single-member group and
   `forget` would silently become a **no-op** — leaving Gap 4 open while
   looking configured. Each lane therefore carries exactly one stable tag.
   Retention was then proven live, not assumed: backdated probe snapshots were
   injected and `forget` correctly selected the older same-day snapshot for
   removal.

3. **One dump serves both destinations.** `pg_dump -Fc -Z0` feeds restic, and
   `pg_restore -f -` converts that *same* archive into the plain `.sql.gz`
   break-glass copy. The off-host and local copies are therefore the same
   consistent snapshot, and the long-standing local restore idiom
   (`gunzip | psql`) is preserved unchanged.

4. **Secrets never reach `docker run` argv.** Credentials are written to a
   mode-600 env file inside a mode-700 workdir and passed via `--env-file`,
   rather than `-e KEY=value` as the superseded `snapshot.sh` did — which
   placed the R2 secret in `/proc/<pid>/cmdline` for the container's lifetime.

5. **The config lane is an explicit allowlist, not an exclude pattern.**
   `.env.bak-*` / `*.bak-rotate-*` hold rotated secrets; an allowlist cannot
   drift open as new suffixes appear, whereas an exclude list must be kept in
   sync forever.

6. **The `chad_hq_standby` standby is on `10.99.0.2` (svdp-dev), not CHAD-HQ.**
   The slot name is legacy. Ground truth came from
   `pg_stat_replication.client_addr`. Before `archive_mode` was flipped, that
   host was checked directly: it carries `standby.signal`, streams from
   `10.99.0.4:5434`, and has **no `restore_command`** (the only occurrence is
   the commented default at `postgresql.conf:279`). Retiring the archive was
   therefore safe, and the slot was confirmed `active` both before and after
   the restart.

### Not done, deliberately

**§5.7 cutover was not executed.** The legacy `snapshot.sh` cron (03:23 UTC),
its local dumps, and the old `s3://obs-glitchtip-backups/droneops/` prefixes
are all still in place, running in parallel. The plan itself requires three
consecutive green days before cutover; that window began 2026-08-17. Exact
commands and criteria are in `PROGRESS.md`.

### Residual operator actions

1. **Cutover after three green days** — see `PROGRESS.md`.
2. **Grafana rule descriptions** for `obs-rule-droneops-backup-stale` still
   tell the operator to run `snapshot.sh` and read `backups/snapshot.log`.
   Correct today; update to `droneops-backup.service` / `journalctl` at
   cutover. Metric names and expressions are unaffected.
3. **The standby on `10.99.0.2` still carries inherited `archive_mode='on'` +
   the same `archive_command` in its `postgresql.auto.conf`.** Inert while it
   is a standby (`on` does not archive during recovery; only `always` does),
   but **a promotion would recreate Gap 7 on that host.** Out of scope here —
   flipping it needs a restart of the standby. Tracked in `ROADMAP.md`.
4. **Open questions 3–5 in the plan** (7-year retention posture, the ~840 MB
   root-owned `n8n_*.sqlite` on droneops-server, legacy volume disposal)
   remain for Bill. Retention shipped at 7 years per D4; it is trivially
   changeable either way.

---

## As-built addendum (2026-08-17) — cold DR rehearsal + review defects

The "Implementation outcome" above records a 12-point matrix run **on BOS-HQ
with BOS-HQ's credentials**. That validates the artifact; it does not validate
the disaster, because in the disaster BOS-HQ and `~/.droneops-secrets/` are
gone. This addendum records the adversarial re-review and the first **cold**
rehearsal, performed from the 1Password Fleet items and the R2 bucket only.

### Outcome: the recovery path is real

Rebuilt on `droneops-server` in a throwaway environment, reading nothing from
BOS-HQ but comparison hashes, touching no production container or volume.
Restic-from-R2, the plain break-glass `.sql.gz`, and live production all agreed
on every content digest. The `.env` restored from the `config` lane matches
live byte-for-byte, and the full 11-service stack renders from restored config
alone — while the same render with `.env` removed is refused on the ADR-0012
`:?` guard, proving the assertion is not vacuous. Step-by-step evidence:
`docs/runbooks/droneops-backup-restore.md` §11.

**The single most valuable result is negative:** the filed secrets were
*sufficient and correct*. A mis-filed recovery key is the failure that DR
discovers too late, and it can only be disproven by reading the vault and
opening the repository with what is actually in it — which is now done.

### D4 (retention) — challenged and upheld

`forget --prune --group-by tags` was re-tested rather than re-reasoned, because
the live repository showed two same-day snapshots per lane surviving a `forget`
that should collapse them. A synthetic 40-day, twice-daily corpus (80 snapshots
of a `db`-tagged lane) converged under the exact production policy to **exactly
20** — 14 daily + weeklies + monthly, one snapshot per day, the 03:23 run
correctly dropped in favour of the 15:23 one. The live anomaly reproduces only
when *every* snapshot in a group is dated today; adding one snapshot dated
yesterday immediately makes the extra same-day one removable. It is a transient
property of a one-day-old repository, **not** a policy fault. D4 stands as
written, and the one-stable-tag-per-lane warning remains load-bearing.

### Defects found (fixed in `c3d9502`)

1. **No concurrency guard** (reliability). The runbook instructs manual runs;
   systemd blocks a second *service* start but not a manual shell run racing a
   timer run. Contention on restic's exclusive lock during `forget --prune`
   would page. Now `flock`-guarded, with the overlap path deliberately exiting
   0 *without* stamping the metric so a benign overlap is silent while a
   persistent one is still caught by `obs-rule-droneops-backup-stale`.
2. **`backups/` not gitignored** (security). Gap 1 moved PII into encrypted R2,
   but the break-glass lane kept writing 1.1 GB of *plaintext* dumps into the
   deploy clone's working tree, untracked and one `git add -A` from a commit.
   The mitigation that makes a lost password survivable was itself an exposure.
3. **The drill never read the `files` lane** (coverage). It asserted `db` and
   `config` and certified "restorable" while never touching the largest lane.
   Now a cross-lane assertion against `tos_acceptances.signed_sha256` — chosen
   over a file-count floor, which stays green against a stale snapshot.
4. **Post-metric error hole** (observability). The local sweep ran after the
   freshness stamp with no `|| fail`, so a failure was visible only in systemd.

Plus a runbook defect with real DR cost: **Procedure A2's first database
command did not work** — `docker compose up -d droneops-standby-db` fails with
`no such service`; the service is `db-standby`. Documented commands in a
recovery runbook are load-bearing and are now executed, not assumed.

### Correcting two hypotheses that did not survive testing

- Bash **does** run the `EXIT` trap on `SIGTERM`, so a `TimeoutStartSec` kill
  does **not** leak the workdir holding the repository password. Tested.
- `fail()` does **not** fail open when ntfy is unavailable: exit 1 confirmed
  with the helper removed, so a dead notification channel cannot manufacture a
  green run.

### Residual (added to the list above)

5. **Repeat the cold, 1Password-only rehearsal annually, and after any rotation
   of either Fleet item.** The quarterly drill cannot detect a mis-filed secret,
   because it never reads 1Password. Only this rehearsal shape can.

---

## Amendment 1 (2026-09-12) — one provider became two; D3's exclusions and D5's RPO stand

Recorded from outside this repo. No DroneOps script, unit, credential, retention setting or
schedule changed, and nothing above is retracted.

This ADR closed seven gaps in a lane that lands in **one Cloudflare account**. R2 has **no
object versioning**, so a delete or corrupted overwrite there replaces the only copy — a gap
this ADR did not address because at the time the fleet had no second provider. noc-master
**ADR-0232** (2026-09-11/12) adds one:

- **`fleetbackup-r2-mirror`** `rclone copy`s (**never `sync`**) every R2 bucket, so
  `droneops-backups` — the same encrypted restic repository — and the legacy
  `obs-glitchtip-backups/droneops/` tree are both copied into Backblaze B2
  `barnardhq-fleet-nightly`. Object Lock **compliance 90 days**, keep-all-versions, **zero
  lifecycle rules, no `forget`, no `prune`**. Because it is copy-forward and never pruned,
  **snapshots D4's `forget --prune` has already removed remain in B2.**
- **`fleetbackup-bos`** takes BOS-HQ's whole root filesystem nightly, so `~/droneops/`,
  `~/.droneops-secrets/restic-droneops.env` and the `droneops_app_data` volume have a copy
  behind a *different* restic password — a second independent route to the credential D1
  identifies as the single unrecoverable secret.

**What this amendment explicitly does NOT change:**

- **D3's exclusions are still correct.** The whole-root lane captures
  `droneops_standby_pgdata` as a filesystem, but only crash-consistently (WAL replay on
  recovery). D3's judgement that *"a physical copy of a running cluster is not a valid
  backup"* stands; the logical `pg_dump -Fc` remains the correct restore artifact.
- **D5's RPO conclusion is unchanged.** The B2 lanes are nightly. RPO stays 12 h from this
  repo's own timer, there is still no PITR, and `pg_receivewal` → a fifth restic lane remains
  the correct path if RPO ever needs to be minutes.
- **D1's recovery-key exposure is unchanged.** The B2 copy of `droneops-backups` is the same
  ciphertext; losing `DroneOps Command Backup Restic Password` still makes it unreadable.

Retention consequence worth naming, since this repository deliberately holds executed TOS
documents and invoice records: nothing in the B2 bucket is purgeable for **90 days**, and it
is never pruned at all — so a deletion obligation that must reach every copy is an operator
retention decision, not a `restic forget` (which the bucket refuses anyway). Restore
procedure: `docs/runbooks/droneops-backup-restore.md` §13 →
`noc-master/docs/runbooks/fleet-b2-backup.md`.
