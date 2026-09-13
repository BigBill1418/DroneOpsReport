# Runbook — DroneOps Command backup & restore

**Decision record:** [ADR-0041](../adr/0041-comprehensive-encrypted-backup-to-r2.md)
**Applies to:** BOS-HQ `10.99.0.4` (`BarnardHQ-BOS`), compose project `droneops`
**Last verified end-to-end:** 2026-08-17

---

## 0. The 60-second summary

Everything is in one **encrypted restic repository** in a dedicated Cloudflare
R2 bucket. Four lanes, distinguished by restic **tag**:

| Tag | What | Restores |
|---|---|---|
| `db` | `pg_dump -Fc` of `droneops` | the database |
| `files` | `/data/uploads` + `/data/reports` from `droneops_app_data` | flight logs, signed TOS PDFs, report deliverables |
| `config` | `.env`, compose files, crontab, volume list, git HEAD | **the ability to boot at all** |
| `legacy` | one-shot pre-migration dump (2026-04-15) | historical HSH-HQ state |
| `legacy-n8n` | one-shot final n8n SQLite DB (2026-04-15; n8n decommissioned 2026-04-21) | historical n8n state — NOT a DroneOps artifact; parked here because this is the fleet's encrypted archive repo. Snapshot `1d0cfb76`, sha256 `ae28fe7b…`, restore-verified 2026-08-17 |
| `legacy-bos-primary-pgdata` | one-shot tar of the orphaned `droneops_postgres_data` volume (BOS pre-promotion primary pgdata; superseded when `droneops-standby-db` was promoted) | historical pgdata; snapshot `66ed2135`, restore-verified 2026-08-18, volume then removed |

```
Repository:  s3:https://<R2_ACCOUNT_ID>.r2.cloudflarestorage.com/droneops-backups/restic
Runner:      restic/restic:0.17.3   (pinned; do NOT use the host's 0.16.4)
Schedule:    droneops-backup.timer — 03:23 + 15:23 UTC (12 h RPO)
Credentials: BOS-HQ ~/.droneops-secrets/restic-droneops.env  (mode 600, dir 700)
```

> ### The two secrets, and which one actually matters
>
> 1Password **Fleet** vault:
> - **`DroneOps Command Backup Restic Password`** — ⚠️ **THE RECOVERY KEY.**
>   Lose it and every backup is permanently unreadable; there is no reset, no
>   escrow, no Cloudflare-side recovery. The on-host copy sits on the machine
>   being backed up and is therefore worthless in the disaster it exists for.
>   **This 1Password copy is the one that matters.**
> - **`DroneOps Command Backup R2 Credentials`** — endpoint, bucket, and the
>   bucket-scoped S3 access key/secret. Rotatable at any time without touching
>   the repository password.
>
> If you have the restic password and the R2 credentials, you can restore
> DroneOps from any machine with Docker and an internet connection. If you have
> only the R2 credentials, you have 737 MiB of undecryptable ciphertext.

> **And since 2026-09-11/12 there is a second copy on a second provider** — not run
> by this repo, not a replacement for anything above. Everything in this runbook
> lands in **one Cloudflare account**, and R2 has **no object versioning**, so a
> delete or a corrupted overwrite there replaces the only copy. See **§13**.

---

## 1. Set up a restic shell (do this first, every procedure below assumes it)

**On BOS-HQ** (credentials already on disk):

```bash
set -a; . ~/.droneops-secrets/restic-droneops.env; set +a
export RESTIC_REPOSITORY="s3:${R2_ENDPOINT}/${R2_BUCKET}/restic"
export AWS_ACCESS_KEY_ID="${R2_ACCESS_KEY_ID}"
export AWS_SECRET_ACCESS_KEY="${R2_SECRET_ACCESS_KEY}"

R() { docker run --rm -i --network host \
        -e RESTIC_REPOSITORY -e RESTIC_PASSWORD -e AWS_ACCESS_KEY_ID -e AWS_SECRET_ACCESS_KEY \
        restic/restic:0.17.3 "$@"; }

R snapshots      # confirm you can read the repo before going further
```

Every procedure below is written in terms of that `R` helper.

**From a bare machine** (nothing but Docker + 1Password), paste the values from
the two Fleet items:

```bash
export RESTIC_REPOSITORY='s3:https://<R2_ACCOUNT_ID>.r2.cloudflarestorage.com/droneops-backups/restic'
read -rsp 'RESTIC_PASSWORD: '      RESTIC_PASSWORD;      echo; export RESTIC_PASSWORD
read -rsp 'AWS_ACCESS_KEY_ID: '    AWS_ACCESS_KEY_ID;    echo; export AWS_ACCESS_KEY_ID
read -rsp 'AWS_SECRET_ACCESS_KEY: ' AWS_SECRET_ACCESS_KEY; echo; export AWS_SECRET_ACCESS_KEY

R() { docker run --rm -i --network host \
        -e RESTIC_REPOSITORY -e RESTIC_PASSWORD -e AWS_ACCESS_KEY_ID -e AWS_SECRET_ACCESS_KEY \
        restic/restic:0.17.3 "$@"; }

R snapshots      # confirm you can read the repo before going further
```

> `read -rsp` keeps the secrets out of your shell history. Do not paste them
> onto a command line.

---

## 2. Procedure A — Full disaster recovery, from nothing

Use when BOS-HQ is gone and you are rebuilding on fresh hardware.

**Order matters: config first.** Restoring the database and uploads onto a new
host without `.env` gets you a stack that will not start — compose refuses on
the ADR-0012 `:?` guards, and every previously-issued JWT is invalid because
`JWT_SECRET_KEY` is gone.

### A1. Restore the configuration

```bash
R restore --tag config latest --target /restore
ls -la /restore/config/
#   .env  docker-compose.yml  docker-compose.override.yml  docker-compose.bos-prod.yml
#   docker-compose.standby.yml  docker-compose.standby.override.yml  .env.example
#   crontab.txt  docker-volumes.txt  git-head.txt
```

```bash
git clone https://github.com/BigBill1418/DroneOpsCommand.git ~/droneops
cd ~/droneops
git checkout "$(cat /restore/config/git-head.txt)"    # the exact commit that was live
cp /restore/config/.env                         ~/droneops/.env
cp /restore/config/docker-compose.override.yml  ~/droneops/
chmod 600 ~/droneops/.env ~/droneops/docker-compose.override.yml
```

`crontab.txt` and `docker-volumes.txt` are reference material — they tell you
what schedules and volumes existed, so you can recreate them deliberately
rather than from memory.

### A2. Bring up Postgres and restore the database

```bash
docker compose up -d db-standby               # SERVICE is `db-standby`; the CONTAINER is `droneops-standby-db`
docker exec droneops-standby-db psql -U droneops -d postgres -c 'CREATE DATABASE droneops'

R dump --tag db latest /droneops.dump > /restore/droneops.dump
docker exec -i droneops-standby-db \
  pg_restore --no-owner --no-privileges --exit-on-error -U droneops -d droneops \
  < /restore/droneops.dump
```

> The `db` lane is a pg_dump **custom** archive (`-Fc`), so this is
> `pg_restore`, **not** `gunzip | psql`. (`-Z0` — uncompressed — is deliberate;
> see ADR-0041 D1. It is what makes restic dedup work.)

> ⚠️ **Service name vs container name.** `droneops-standby-db` is the
> `container_name`; the compose **service** is `db-standby`. `docker compose`
> subcommands take the service (`up -d db-standby`), `docker exec` takes the
> container (`docker exec droneops-standby-db …`). Mixing them fails with
> `no such service: droneops-standby-db` — this runbook said the wrong one
> until the 2026-08-17 DR rehearsal caught it (§11).

### A3. Restore the file volume

```bash
docker volume create droneops_app_data
R restore --tag files latest --target /restore/files
docker run --rm -v droneops_app_data:/d -v /restore/files/data:/src:ro alpine:3 \
  sh -c 'cp -a /src/uploads /src/reports /d/ && chown -R 1000:1000 /d'
```

### A4. Start the stack and verify

```bash
cd ~/droneops && docker compose up -d
docker exec droneops-standby-db psql -U droneops -d droneops -tAc 'SELECT count(*) FROM flights'
curl -sf https://droneops.barnardhq.com/health
```

Sanity floor: `flights` should be within one backup cycle of the last known
count (780 as of 2026-08-17). A near-empty restore that "succeeded" is the
failure mode the drill exists to catch — treat it as a failed restore.

---

## 3. Procedure B — Database-only rollback

Use when the data is wrong (bad migration, bad bulk edit) but the host is fine.

```bash
R snapshots --tag db                       # pick the snapshot from BEFORE the damage
R dump <snapshot-id> /droneops.dump > /tmp/rollback.dump

# Rehearse into a scratch DB first. Never restore straight over live.
docker exec droneops-standby-db psql -U droneops -d droneops -qAtc \
  'DROP DATABASE IF EXISTS droneops_rollback WITH (FORCE)'
docker exec droneops-standby-db psql -U droneops -d droneops -qAtc \
  'CREATE DATABASE droneops_rollback'
docker exec -i droneops-standby-db \
  pg_restore --no-owner --no-privileges --exit-on-error -U droneops -d droneops_rollback \
  < /tmp/rollback.dump
docker exec droneops-standby-db psql -U droneops -d droneops_rollback -c \
  'SELECT count(*) FROM flights'
```

Only once the scratch copy looks right, cut over — stop the app containers
first so nothing writes during the swap:

```bash
docker compose stop backend worker beat flight-parser
docker exec droneops-standby-db psql -U droneops -d postgres -qAtc \
  'ALTER DATABASE droneops RENAME TO droneops_preroll_'"$(date -u +%Y%m%d%H%M)"
docker exec droneops-standby-db psql -U droneops -d postgres -qAtc \
  'ALTER DATABASE droneops_rollback RENAME TO droneops'
docker compose start backend worker beat flight-parser
```

Keep `droneops_preroll_*` until you are certain. Drop it deliberately, later.

> ⚠️ **The CHAD-HQ standby replicates deletes.** A replica is not a backup.
> Rolling back the primary does not roll back anything you need from the
> standby, and the standby will faithfully replicate whatever you do here.

---

## 4. Procedure C — Single-file recovery from `uploads/`

The common real case: one flight log or signed PDF was overwritten, truncated,
or deleted. This is what the `files` lane's snapshot history is *for* — the old
`aws s3 sync` mirror could not do it, because the next sync copied the damage
over the only good copy (ADR-0041 Gap 3).

```bash
# Find which snapshots contain a good copy
R find --tag files 'DOC-20260503062353-f332cda9.pdf'

# Inspect a specific snapshot
R ls <snapshot-id> /data/uploads/tos_signed | head

# Pull one file to stdout — snapshot paths are /data/..., NOT the live mount path
R dump <snapshot-id> /data/uploads/tos_signed/DOC-20260503062353-f332cda9.pdf > /tmp/recovered.pdf
sha256sum /tmp/recovered.pdf

# Put it back
docker run --rm -v droneops_app_data:/d -v /tmp:/in:ro alpine:3 \
  sh -c 'cp /in/recovered.pdf /d/uploads/tos_signed/ && chown 1000:1000 /d/uploads/tos_signed/recovered.pdf'
```

> **Path gotcha.** Inside the backup, files live under `/data/...` because the
> volume is mounted at `/data` when the snapshot is taken. If you inspect the
> live volume mounted somewhere else (e.g. `-v droneops_app_data:/d`), the live
> path is `/d/...` but the *snapshot* path is still `/data/...`. Using the
> wrong prefix makes `restic dump` write **zero bytes** and exit — and a
> zero-byte "restore" is easy to mistake for success. Always check the size.

---

## 5. Procedure D — Break-glass: restore WITHOUT the restic password

If `RESTIC_PASSWORD` is lost, the R2 repository is unreadable. The retained
local plain dumps on BOS-HQ are the fallback, and they need no key at all.
This is why the local lane is deliberately kept (ADR-0041, Option C).

```bash
ls -lt ~/droneops/backups/droneops-*.sql.gz | head    # 14 days of retention
gunzip -t ~/droneops/backups/droneops-20260817-064121.sql.gz    # verify first

docker exec droneops-standby-db psql -U droneops -d postgres -c 'CREATE DATABASE droneops_bg'
gunzip -c ~/droneops/backups/droneops-20260817-064121.sql.gz \
  | docker exec -i droneops-standby-db psql -U droneops -d droneops_bg -v ON_ERROR_STOP=1
```

> These are **plain SQL** (`psql`), not custom archives — the opposite of the
> `db` lane. They are also **on the machine being backed up**, so they cover a
> lost password, not a lost host. Scope: database only, 14 days, no uploads, no
> config.
>
> **If you ever actually need this, treat it as an incident:** immediately
> generate a new restic password, `restic init` a fresh repository, and re-file
> the password to 1Password. Do not keep running on the local-only lane.

---

## 6. Procedure E — Run the drill by hand

The drill is what converts "a backup exists" into "a backup restores". It runs
quarterly (16th of Jan/Apr/Jul/Oct, 16:23 UTC) and is safe to run any time — it
restores into a throwaway DB and drops it via a `trap`, even on failure.

```bash
sudo systemctl start droneops-restore-drill.service     # as scheduled
# or, watched:
cd ~/droneops && ./scripts/restore-drill.sh
```

It asserts, and fails loudly on any of:

- newest `db` snapshot is **< 48 h old** (a stale backup is not certified);
- the artifact is a valid pg_dump custom archive;
- restored `flights` ≥ **90 %** of live, `battery_logs` / `tos_acceptances` non-empty;
- **`.env` restored from the `config` lane sha256-matches the live file** —
  the single assertion that proves ADR-0041 Gap 5 stays closed.

On success it stamps `droneops_restore_drill_last_success_timestamp_seconds`
and posts one `default` ntfy note.

### Run a backup by hand

```bash
cd ~/droneops && ./scripts/droneops-backup.sh     # watched, foreground
sudo systemctl start droneops-backup.service      # through systemd (unit env)
journalctl -u droneops-backup.service -n 50
```

---

## 7. Monitoring — what pages, and what it means

| Signal | Rule | Meaning |
|---|---|---|
| `droneops_backup_last_success_timestamp_seconds` | `obs-rule-droneops-backup-stale` — >28 h **or** `absent()*9999`, `severity: high` | No fully successful backup in over a cycle. The `absent()` clause is the dead-man: an unset gauge would otherwise read permanently green. |
| `droneops_restore_drill_last_success_timestamp_seconds` | `obs-rule-droneops-restore-drill-stale` — >100 days, `severity: default` | The drill is dead or failing silently; backups are no longer restore-proven. |
| ntfy `[DroneOps Command] nightly backup FAILED` | `high` on `infrawatch-alerts`, 6 h cooldown | A run failed. Click → `noc-mastercontrol.barnardhq.com/status/droneops`. |

> ⚠️ **Both metric names are a hard contract** with
> `/opt/infrawatch/grafana/provisioning/alerting/observability-alerts.yml`.
> Renaming either without editing that file **in the same change** converts a
> live alert into a permanently-green dead man — the exact failure this lane
> exists to prevent.

The metric is stamped **only** after every lane, the retention pass and the
integrity check have all succeeded. A partial run does not advance it.

---

## 8. Triage — a failed backup

Work down this list; each step is cheap.

1. **Read the actual error.** `journalctl -u droneops-backup.service -n 80`
   (or `tail ~/droneops/backups/snapshot.log` for the legacy lane).
2. **Is the DB container up?** `docker ps | grep droneops-standby-db`. The
   script refuses to run without it.
3. **Credentials.** A `SignatureDoesNotMatch` / `BucketExists` failure means
   the R2 key is wrong or rotated. Compare
   `~/.droneops-secrets/restic-droneops.env` against the 1Password item.
4. **Stale lock.** If a previous run was killed, restic may hold a lock:
   `R unlock` (safe — it only removes locks with no live process).
5. **Repository integrity.** `R check`. If this fails, do **not** prune; capture
   the output and investigate before any destructive operation.
6. **Disk.** The db lane stages a ~460 MB uncompressed dump under `/tmp`.
   `df -h /`.
7. **Re-run watched** and confirm the metric advances:
   `cat /var/lib/node_exporter/textfile_collector/droneops_backup.prom`.

**Do not "fix" a failing backup by disabling the alert.** The alert is the only
thing standing between a broken backup and finding out during a restore.

---

## 9. Retention, cost, and what is deliberately *not* backed up

```
forget --prune --keep-daily 14 --keep-weekly 8 --keep-monthly 24 --keep-yearly unlimited --group-by tags
```

`--group-by tags` is **required** — without it the policy mixes all four lanes
together and prunes `config`/`legacy` wrongly.

> ⚠️ Each lane carries exactly **one** stable tag. Do **not** add a per-run date
> tag: `--group-by tags` groups on the *full* tag set, so `db,2026-08-17` and
> `db,2026-08-18` would each become a single-member group and `forget` would
> silently become a **no-op** — leaving retention unenforced while looking
> perfectly configured.

24-month / 7-year retention is driven by content, not convention: this
repository holds executed TOS documents and invoice records.

**Deliberately excluded** (recorded so nobody re-litigates them — ADR-0041 D3):
`droneops_ollama_data` (4.6 GB of re-pullable model weights),
`droneops_postgres_data` (neutralized legacy primary), `/data/backups` (stale
in-app dumps — deleted, not archived), `droneops-gw_caddy_data` (ACME state,
self-regenerating), Redis (ephemeral Celery broker), the `droneops-demo` stack,
and `droneops_standby_pgdata` **as a filesystem** — a physical copy of a running
cluster is not a valid backup; the logical dump is the correct artifact.

Cost is roughly **$0.03–0.10/month**. Storage is not a design constraint here,
which is why 7-year retention is the affordable choice rather than an
extravagant one.

---

## 10. RPO / RRO, honestly stated

- **RPO = 12 h** (03:23 + 15:23 UTC). A crash loses at most one half-day of
  writes — on the order of ~1 MB and ~3 flights, each of which has a
  re-uploadable source on the controller (ADR-0002 / DroneOpsSync ingest).
- **There is no PITR.** WAL archiving was retired on 2026-08-17: it wrote into
  the very volume it was meant to protect, had never been pruned (5.5 GiB /
  358 segments), and had not archived since 2026-07-22 — a 26-day hole. It was
  a liability wearing PITR's clothes, not a capability. See ADR-0041 D5.
- **If RPO ever needs to be minutes**, the correct path is `pg_receivewal` →
  a dedicated volume → a fifth restic lane. It is **not** re-enabling
  `archive_command` into pgdata.
- **Restore time** is dominated by the 460 MB db lane and 657 MB files lane:
  expect **minutes**, not hours, over a normal link.

---

## 11. DR rehearsal — 2026-08-17 (cold, from 1Password only)

The quarterly drill (§6) runs **on BOS-HQ, using the credentials on BOS-HQ**.
That proves the artifact restores; it proves nothing about the disaster it
exists for, because in that disaster BOS-HQ and `~/.droneops-secrets/` are
gone. This rehearsal closed that hole by rebuilding from the **only two inputs
that survive a lost host**: the 1Password Fleet items and the R2 bucket.

**Executed on** `droneops-server` (HSH-HQ), in a throwaway environment.
**Nothing on BOS-HQ was written to, and no production container or volume was
touched** — prod was read-only, and only to produce comparison hashes.

### What was rehearsed, and what it proved

| # | Step | Evidence |
|---|---|---|
| 1 | Read both secrets from 1Password via the `op` service account — **no BOS-HQ input** | Parsed by label into a mode-600 env file. Shapes: restic password 64 ch, `AWS_ACCESS_KEY_ID` 32 ch, `AWS_SECRET_ACCESS_KEY` 64 ch |
| 2 | Open the repository with those secrets alone | `restic snapshots` exit 0; `repo_id=e9c833aef19c…` — matches the ID recorded in the 1Password note |
| 3 | Restore **all four lanes** to scratch | db 483,440,022 B · files 226 files / 657 MiB · config 10 files · legacy 25,963,218 B — all exit 0 |
| 4 | `pg_restore` the db lane into a throwaway `postgres:16-alpine` (matching prod 16.14), no published port, dedicated volume | exit 0 in **22 s** |
| 5 | Row counts vs prod | flights **780**, battery_logs **779**, tos_acceptances **10**, customers **7**, aircraft 9, invoices 6 — all match |
| 6 | **Deep probe** — payload volume and content digests, run identically against restored and live | `telemetry_bytes=146,420,719` · `gps_track_bytes=334,757,775` · `sum_point_count=6,842,636` · `flights_digest=4d6d9276…` · `tos_digest=2c106a61…` — **byte-identical to live on every probe** |
| 7 | Config lane completeness | `.env` sha256 `8839dd0d…` **matches live exactly**; 41 unique keys, trailing newline present, no empty values |
| 8 | **Can the stack actually boot?** | All 11 services render from restored config alone (`docker compose config`). **Negative control:** the same render with `.env` removed is *refused* — `required variable DATABASE_URL is missing a value … (no default — see ADR-0012)`. Gap 5 is closed, and the check is not vacuous |
| 9 | Files lane vs prod | **All 226 files sha256-identical to production** |
| 10 | Files lane vs the *database* | All 10 executed TOS PDFs match `tos_acceptances.signed_sha256` byte-for-byte — an independent, cross-lane proof |
| 11 | Legacy lane readable | Valid archive, 83 TOC entries, `Archive created at 2026-04-15 02:00:02 UTC` |
| 12 | **Break-glass (§5) with no restic password at all** | `gunzip \| psql` of the plain `.sql.gz` produced digests **identical** to both the restic path and live prod |
| 13 | Teardown | Container + volume destroyed; restored `.env`, dump and env file `shred`ded; scratch confirmed free of secret material |
| 14 | Scheduled path still works after the fixes | `systemctl start droneops-backup.service` → `Result=success`, 56 s, metric advanced `1786949417 → 1786951740` (2026-08-17 00:29 PDT), new snapshots, `restic check` clean, lock released, no `/tmp` residue |

**Verdict: recovery works cold.** Three independent paths (restic-from-R2,
break-glass `.sql.gz`, live prod) agree on every digest. The 1Password items
alone are sufficient — nothing needed from the failed host.

### What the rehearsal broke that a drill would not have

- **`docker compose up -d droneops-standby-db` (Procedure A2) does not work** —
  `no such service`. The service is `db-standby`. This was the *first command*
  of the database step in a from-nothing recovery. Fixed above.
- The quarterly drill **never read the `files` lane** — 657 MiB of flight logs,
  report deliverables and executed TOS PDFs, the largest lane. It certified
  "restorable" while never touching it. Fixed in `restore-drill.sh` with a
  cross-lane sha256 assertion (commit `c3d9502`).

### Known cosmetics (not defects, recorded so they are not re-investigated)

- `.env` contains **`FRONTEND_URL` twice** (lines 24 and 42) — 42 assignments,
  41 unique keys. Last wins; harmless, but worth de-duplicating.
- `uploads/tos_signed/` holds **11** PDFs against **10** `tos_acceptances`
  rows. `DOC-20260503173725-5b448dbe.pdf` has no DB row — an orphan from an
  abandoned signing flow. It is backed up correctly.
- 14 of 780 flights have a `gps_track` that is neither a JSON object nor array;
  identical in prod and restored, so not a restore artifact.

### Next rehearsal

The drill (§6) covers the artifact quarterly. **Repeat *this* cold, 1Password-only
rehearsal annually, or after any rotation of either Fleet item** — a rotation
that is filed incorrectly is invisible until exactly the moment it matters.

---

## 12. Related

- [ADR-0041](../adr/0041-comprehensive-encrypted-backup-to-r2.md) — the decision, the seven gaps, options considered
- §13 below / noc-master ADR-0232 — the fleet's second storage provider
- `scripts/droneops-backup.sh` — the backup job
- `scripts/restore-drill.sh` — the quarterly proof
- `scripts/systemd/droneops-backup.{service,timer}`, `droneops-restore-drill.{service,timer}`
- Fleet ADR-0036 (ntfy transport) / ADR-0037 (notification noise policy) / ADR-0086 (1Password Fleet vault)

---

## 13. The second provider: Backblaze B2 (noc-master ADR-0232) — the fleet runs this, not DroneOps

**Pointer, not a duplicate.** Doctrine:
`noc-master/docs/adr/0232-second-provider-immutable-nightly-backup.md`. Operations:
`noc-master/docs/runbooks/fleet-b2-backup.md`. **Nothing in §§0–12 changed** — the four
tagged lanes, the twice-daily timer, the `forget --prune` policy, the freshness metric,
`obs-rule-droneops-backup-stale` and the quarterly drill are all untouched and remain the
primary path.

Since 2026-09-11/12, two fleet lanes give DroneOps state a copy on a **different storage
provider**, nightly, into one Backblaze B2 bucket `barnardhq-fleet-nightly` — Object Lock
**compliance mode, 90 days**, keep-all-versions, **zero lifecycle rules, no `forget`, no
`prune`, ever**:

| Lane | What it covers for DroneOps | How it comes back |
|---|---|---|
| `fleetbackup-bos` (BOS-HQ, 09:15 PT) | BOS-HQ's **whole root filesystem** — so `~/droneops/` (compose files, `.env`, scripts), **`~/.droneops-secrets/restic-droneops.env`**, the `droneops_app_data` volume (`/data/uploads` + `/data/reports`) and the docker volumes generally, under `/var/lib/docker/volumes` (verified not excluded — only container image/layer stores are) | `restic restore latest --target /var/tmp/… --include <path>` with the lane env on BOS, or from **any** machine using the 1Password values. Fleet runbook §2a |
| `fleetbackup-r2-mirror` (BOS-HQ, 07:00 PT) | `rclone copy` — **never `sync`** — of **every** R2 bucket, list read live from the R2 API each run. So the **`droneops-backups` repository itself** is copied, and so is the legacy `obs-glitchtip-backups/droneops/` tree. A delete in R2 does not propagate | `rclone copy b2:barnardhq-fleet-nightly/fleetbackup-r2-mirror/droneops-backups/<key> …`, then use it as an ordinary restic repo with **the same restic password** (§0). Fleet runbook §2c |

Two things this genuinely buys, stated precisely:

- **The `forget --prune` in §9 no longer decides what exists forever.** Because the mirror
  is copy-forward and B2 is never pruned, **snapshots this repo has already forgotten
  remain in B2.** That is the feature, and it is also a cost line that only rises.
- **The recovery key problem is unchanged and still the one that matters.** The B2 copy of
  `droneops-backups` is the *same encrypted restic repository* — `DroneOps Command Backup
  Restic Password` is still the single point of unrecoverable failure, and a second
  provider does nothing about that. What the `fleetbackup-bos` lane *does* add is a copy of
  `~/.droneops-secrets/restic-droneops.env` itself, behind a **different** password
  (`Fleet B2 nightly — lane bos`) — a second, independent route to the same secret. Both
  are in the 1Password Fleet vault; neither is on a host you might have lost.

Three things it does **not** buy — do not let the table above be read as more than it is:

- **It does not make `droneops_standby_pgdata` a valid backup.** §9 excludes the running
  cluster's pgdata as a filesystem on purpose, and that judgement stands: the whole-root
  lane *does* capture that volume, but only as a crash-consistent filesystem copy that
  recovers via WAL replay. **The logical `pg_dump -Fc` in the `db` lane is still the
  correct artifact** — restore from it, not from the volume, unless the dump is gone.
- **It is not PITR and it does not change RPO.** §10 stands: RPO is 12 h from this repo's
  own lanes; the B2 lane is once a night.
- **It is file-level, not bare-metal.** `/boot` and container image layers are excluded, so
  an image built on BOS and never pushed to a registry is not in there.

Health: **one** digest a day at 11:00 PT on ntfy `infrawatch-alerts` (the DroneOps backup
topics are untouched); weekly `restic check`; quarterly restore drills on the 16th of
Jan/Apr/Jul/Oct. **Never** `restic forget`/`prune` a `fleetbackup-*` repository, `rclone
sync` the mirror, or add a lifecycle rule to that bucket — the full list is fleet runbook
§9. And note: nothing in that bucket is purgeable for 90 days, so since this repository
holds **executed TOS documents and invoice records**, a deletion obligation that must reach
every copy is a retention question for the operator rather than a `restic forget`.