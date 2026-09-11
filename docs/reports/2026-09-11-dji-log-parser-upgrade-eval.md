# P-EVAL — `dji-log-parser` before/after evaluation (2026-09-11)

Executes phase **P-EVAL** of `docs/plans/2026-09-04-flight-details-data-ingestion.md`
§6, which exists to satisfy operator decision **D6** in
[ADR-0043](../adr/0043-flight-details-sidecar-table-for-extended-log-data.md):
*evaluate the library bump first, with a before/after diff over all retained
logs; adopt only if nothing moves unexplained.*

No database row was written, no schema changed, no container restarted, and
`flight-parser/Cargo.toml` was **not** modified. The deliverable is evidence.
All times Pacific unless labelled UTC.

---

## 0. Recommendation — **DO NOT ADOPT. There is nothing to adopt.**

**The newest published `dji-log-parser` is `0.5.7`, which is exactly what
`flight-parser/Cargo.lock` already pins.** D6 contemplates a "library bump"; as
of 2026-09-11 no such release exists. Upstream's last release was 2025-04-26 —
**16½ months ago** — and its repository has had no commit since 2025-06-07.

The only artefact newer than the pin is upstream `master` at commit
**`88fcfc96d2fcd29c6c69e9643196a37c4feb3888`**, one commit ahead of `v0.5.7`,
which parses **Inspire 1 battery serial numbers**. I evaluated it anyway, as the
candidate, because it is the only candidate that exists. Result:

- **782 comparisons over 762 distinct real DJI logs, 24 metrics each, every
  `gps_track` coordinate included: zero differences.** Not "within tolerance" —
  bit-identical, including one-ULP float comparison.
- Its only behavioural change is confined to `ProductType::Inspire1` /
  `Inspire1Pro` / `Inspire1RAW`. **The fleet operates no Inspire 1**, so the
  change is unreachable here by construction.
- Adopting it would convert a checksummed crates.io dependency into a git
  dependency pinned to an unreleased commit — a strictly worse supply-chain
  posture — in exchange for no measurable benefit.

**So: stay on `0.5.7`.** The decision D6 guards is not "is the new version
safe"; it is "does a new version exist." It does not.

### What that settles for the phases waiting on this gate

| Question P-EVAL was asked | Answer | Consequence |
|---|---|---|
| Does a newer crate fix `ProductType`, so `Unknown(NNN)` resolves? | **No.** The enum is unchanged; its newest members are still `Avata2` / `Matrice350RTK`. All four placeholders the fleet produces persist: `Unknown(178)` Matrice 4TD, `Unknown(137)` Mavic 4 Pro, `Unknown(139)` Mini 5 Pro, `Unknown(150)` Matrice 4T. | **P1's `aircraft_name` fallback and P4(b)'s repair predicate are both still required, in full.** Nothing shrinks. |
| Does a newer crate fix `SmartBatteryStatic`, so §2.4's shim can be dropped? | **No.** `record/smart_battery_group.rs` is byte-for-byte identical between the pin and the candidate. | **P2 must build the shim.** §2.4's characterisation is correct — and §4 below refines its limits. |
| Should P2 be re-planned around a different crate? | **No.** | **P2 is unblocked and should proceed on `0.5.7`.** |

Four follow-ups fall out of the evidence and are listed in §8. The one worth
reading first: **`flight-parser/Cargo.toml` does not pin `0.5.7` at all** — it
requests `"0.5"`, so the lockfile is the only thing enforcing D6.

---

## 1. Resolving the candidate — two independent primary sources

The plan (§6 step 1) says *do not assume a version number*. Both checks agree.

**crates.io API**, queried 2026-09-11:

```
max_version       : 0.5.7
newest_version    : 0.5.7
max_stable_version: 0.5.7
updated_at        : 2025-04-26T13:02:44.575301Z
repository        : https://github.com/lvauvillier/dji-log-parser

0.5.7   2025-04-26   yanked=False
0.5.6   2025-03-20   yanked=False
0.5.5   2025-02-10   yanked=False
0.5.4   2024-11-03   yanked=False
```

**`cargo search`**, run inside `rust:1.85-bookworm` — the toolchain
`flight-parser/Dockerfile` pins:

```
rustc 1.85.1 (4eb161250 2025-03-15)
cargo 1.85.1 (d73d2caf9 2024-12-31)
    Updating crates.io index
dji-log-parser = "0.5.7"    # Library for parsing DJI txt logs
```

**What the repo pins today.** The plan says `0.5.7` is pinned. That is true of
the lockfile and **not** of the manifest:

```
flight-parser/Cargo.toml : dji-log-parser = "0.5"       ← a caret range, ^0.5.0
flight-parser/Cargo.lock : version = "0.5.7"
                           checksum = "13d15112d03932c3a4122c43db9604ec4378a41621193b29fb8f8b2e8502ba0b"
```

Today that distinction is inert, because `0.5.7` *is* the maximum of `^0.5`.
It stops being inert the moment upstream publishes `0.5.8` (see §8.1).

### The candidate: upstream `master`, one commit ahead

```
default_branch : master        pushed_at : 2025-06-07T10:02:17Z
tags           : v0.5.7 e2e0775670, v0.5.6 c9de137d77, v0.5.5 2222bc418b, …
compare v0.5.7...HEAD → status: ahead  ahead_by: 1  total_commits: 1
   88fcfc96d2  2025-06-07  Parse Inspire 1 battery serial numbers (#28)
   FILE modified dji-log-parser/src/layout/details.rs  +36/-1
   FILE modified dji-log-parser/src/record/recover.rs  +5/-2
```

Upstream is **dormant**: one commit in 15 months, 20 open issues, 116 stars,
last release 2025-04-26. That dormancy is itself a finding (§8.3).

---

## 2. What the candidate commit actually changes — read from the diff

Both edited sites replace an inline UTF-8 decode of `battery_sn` with a call to
a new `parse_battery_sn(product_type, buf)`:

```rust
-    #[br(count = if version <= 5 { 10 } else { 16 }, map = |s: Vec<u8>| String::from_utf8_lossy(&s).trim_end_matches('\0').to_string())]
+    #[br(count = if version <= 5 { 10 } else { 16 })]
+    #[br(temp)]
+    battery_buf: Vec<u8>,
+    #[br(calc = parse_battery_sn(product_type, battery_buf))]
     pub battery_sn: String,
```

```rust
pub fn parse_battery_sn(product_type: ProductType, buf: Vec<u8>) -> String {
    const BCD_PRODUCTS: [ProductType; 3] = [
        ProductType::Inspire1, ProductType::Inspire1Pro, ProductType::Inspire1RAW,
    ];
    if BCD_PRODUCTS.contains(&product_type) {
        decode_reversed_bcd_battery_sn(buf)
    } else {
        String::from_utf8_lossy(&buf).trim_end_matches('\0').to_string()
    }
}
```

Two properties make this commit safe to reason about *before* measuring it, and
both were then confirmed empirically in §5:

1. **Byte consumption is unchanged.** The `count` expression is identical on
   both sides (`if version <= 5 { 10 } else { 16 }`). `binrw` reads the stream
   sequentially, so an unchanged byte count means **no field after `battery_sn`
   can shift offset.** This is the property that would otherwise let a header
   edit silently move `total_distance`, `max_height` or `latitude`.
2. **The new behaviour is gated on `product_type`.** For every airframe that is
   not an Inspire 1, the `else` branch is the *identical* expression the pinned
   version uses. So the change is a no-op for any other product type.

Neither `ProductType` nor `record/smart_battery_group.rs` is touched by the
commit — which is what answers the two questions P2 was waiting on.

---

## 3. `ProductType` — no `Unknown(NNN)` resolves

The enum at the candidate commit ends:

```
… Mini3Pro, Mavic3Pro, Mini2SE, Matrice30, Mavic3Enterprise, Avata, Mini4Pro,
  Avata2, Matrice350RTK,
  #[serde(untagged)]
  Unknown(u8),
```

There is no `Mavic4Pro`, no `Matrice4TD`, no `Matrice4T`, no `Mini5Pro`. Every
placeholder the fleet produces is still `Unknown(NNN)`. Measured against the
production database (`droneops-standby-db`, read-only):

| `product_type` | header `aircraft_name` | flights | resolves on candidate? |
|---|---|---:|---|
| `Unknown(178)` | Matrice 4TD | 79 | no |
| `Unknown(137)` | DJI Mavic 4 Pro | 56 | no |
| `Avata2` | DJI Avata 2 | 34 | n/a — already named |
| `Unknown(139)` | BigThingsSmallPackages (Mini 5 Pro) | 31 | no |
| `Mavic3Pro` | Badass V.2 / Bad Mother Fucker | 16 | n/a |
| `Matrice30` | Maverick | 8 | n/a |
| `FPV` | DJI FPV | 2 | n/a |

A fourth placeholder, **`Unknown(150)` = Matrice 4T**, appears on 39 of the
recovered ODL-era logs and on no native `dji_txt` row yet — it becomes live when
P7 re-imports (§8.3).

**166 of 226 DJI flights — 73% — are on an airframe the crate cannot name**, and
upstream has added no product type in 15 months. `dji.rs`'s
`Unknown(NNN) → aircraft_name` fallback is therefore **permanent
infrastructure, not a stopgap**, and P4(b)'s repair pass has the same standing.

---

## 4. `SmartBatteryStatic` — the §2.4 shim is still needed, and its limit is now known

`record/smart_battery_group.rs` is **identical** between `0.5.7` and the
candidate, so nothing upstream has fixed this. The struct, read from the
candidate:

```rust
pub enum SmartBatteryGroup {
    #[br(magic = 1u8)] SmartBatteryStatic(SmartBatteryStatic), …
}
pub struct SmartBatteryStatic {
    pub index: u8,
    pub designed_capacity: u32,
    pub loop_times: u16,
    pub full_voltage: u32,
    …
}
```

**§2.4's diagnosis is correct, and sharper than "big-endian-wrong."** Working
the census's own recorded values against this layout, every field after `index`
is read **exactly one byte early** — uniformly, not cumulatively. (The plan's
table has one transcription slip: `1505282` is `0x16F802`, not `0x16F002`.)

| Field | Raw | Bytes read (LE) | True bytes | `raw >> 8` | Expected |
|---|---|---|---|---|---|
| `designed_capacity` (M4TD) | 1899520 | `00 FC 1C 00` | `FC 1C 00 00` | 7420 | 7420 mAh |
| `designed_capacity` (M30) | 1505282 | `02 F8 16 00` | `F8 16 00 00` | 5880 | 5880 mAh |
| `loop_times` (M4TD) | 5888 | `00 17` | `17 00` | 23 | 23 |
| `loop_times` (M30) | 2304 | `00 09` | `09 00` | 9 | 9 |

The "stolen" low byte (`0x00`, `0x02`) is a byte the struct never declares —
consistent with **C struct alignment padding after the `u8 index`**, which is
why it is uninitialised rather than always zero. An endian swap does not fit
either observation; a one-byte offset fits both exactly. So `raw >> 8` is right
and `swap_bytes()` is wrong, as §2.4 says.

### The limit P2 must carry (new finding)

A one-byte-early read of an *N*-byte little-endian field yields the true value's
low *N−1* bytes. `raw >> 8` therefore recovers the true value **if and only if
the true most-significant byte is zero**:

| Field | Shim is exact while | Headroom |
|---|---|---|
| `designed_capacity` (u32, mAh) | value < 2²⁴ = 16,777,216 | never a problem |
| `full_voltage` (u32, mV) | value < 16,777 V | never a problem |
| **`loop_times` (u16, cycles)** | **value ≤ 255** | **breaks at 256 cycles** |

**This matters and the plan's plausibility gate cannot catch it.** §2.4 gates
cycles to `0..=3000`. A pack at a true 260 cycles reads as `0x?? 04` → `>> 8`
→ **4**, which sits comfortably inside `0..=3000`, is stored as plausible, and
feeds D4's `batteries.cycle_count`. Silent, plausible, and wrong — and it gets
*worse* with pack age, wrapping every 256 cycles.

The census's observed values (23, 9) are both under 255, so nothing is wrong
today. M30/TB30 and M4TD packs in regular service will cross 255.
**Recommendation for P2-a:** read `loop_times` from the record payload at the
corrected offset rather than post-hoc shifting, or — if the shift is kept —
record `pack_cycle_count_may_have_wrapped` and refuse to let a *decreasing*
observed cycle count feed `batteries.cycle_count` (the `GREATEST` monotonic
guard §1.6 already specifies happens to absorb this, which is worth stating as
the reason it exists rather than leaving it to luck).

Per §6's scope split, the pack-value comparison itself belongs to P2's records
spike; the above is read from the crate source, **not measured on a log**, and
is labelled as such.

---
## 5. The measurement — 782 comparisons, zero differences

### 5.1 The harness

`tools/p-eval/` — a standalone crate, **not** part of the `flight-parser` build,
never deployed, no database access of any kind. It links **both** crate versions
in one binary:

```
tools/p-eval/Cargo.lock
  name = "dji-log-parser"  version = "0.5.7"
  source   = "registry+https://github.com/rust-lang/crates.io-index"
  checksum = "13d15112d03932c3a4122c43db9604ec4378a41621193b29fb8f8b2e8502ba0b"   ← identical to flight-parser/Cargo.lock

  name = "dji-log-parser"  version = "0.5.7"
  source = "git+https://github.com/lvauvillier/dji-log-parser?rev=88fcfc96d2fcd29c6c69e9643196a37c4feb3888#88fcfc96…"
```

```
   Compiling dji-log-parser v0.5.7
   Compiling dji-log-parser v0.5.7 (https://github.com/lvauvillier/dji-log-parser?rev=88fcfc96d2…#88fcfc96)
   Compiling p-eval v0.1.0 (/repo/tools/p-eval)
    Finished `release` profile [optimized] target(s) in 1m 47s
```

The **pinned arm's crates.io checksum is identical to `flight-parser/Cargo.lock`'s**,
so the "before" reading is produced by the exact crate bytes production links —
not a re-resolved approximation.

Three design choices matter to whether the result can be trusted:

1. **One binary, one pass, one keychain.** Both readings come from the same file
   bytes and the *same* keychain object. Two separate builds would have needed
   two DJI keychain round-trips per log, doubling load on DJI's API and adding a
   variable the experiment is trying to hold constant.
2. **`gate.rs` is production's file, included verbatim** via
   `#[path = "../../../flight-parser/src/gate.rs"]` — so the one piece of real
   branching logic (the ADR-0028 C1 outlier gate) cannot drift from what
   production runs. Its own 7 tests run inside the harness's suite and pass,
   which is how we know the real file was included and not a stub.
3. **Both readings are generated from one macro body**, so anything that differs
   in the output came from the crate, not from the harness.

**Floats are compared bitwise (`f64::to_bits`), not with a tolerance.** §6's
adoption rule is "bit-identical"; a tolerance would quietly absorb exactly the
sub-epsilon drift a dependency change is most likely to produce.

Every `gps_track` coordinate is covered by a SHA-256 digest taken over the
ordered, full-precision `(lat, lng, alt, speed, heading)` tuples, with the index
of the first divergence recorded if the digests disagree. That is every
coordinate, exactly, with bounded output — **6,756,743 coordinates compared.**

### 5.2 The corpus — re-counted, not inherited

The plan's §6 step 3 says "the 182 real files"; §8's 2026-09-05 correction says
190 real; §8 also says *"re-derive from the database rather than from any figure
written here."* Doing that on 2026-09-11:

| Set | Count | How established |
|---|---:|---|
| Files in `droneops-backend-1:/data/uploads/flight_logs` | **200** | `ls -1 \| wc -l` |
| …of which are the dummy test files | **2** | 40 B / 70 B, magic `64 75 6d 6d` = `"dumm"`, **not** in the DB |
| …real DJI logs | **198** | magic `29 03 00 00`; hash-set intersection with `flights.source_file_hash` = **198** |
| `dji_txt` rows in the prod DB | **226** | plan says 210; §8 correction says 218; 8 more arrived since |
| `dji_txt` rows with **no** retained file | **28** | unchanged from §8 — still unrecoverable |
| Recovered ODL-era originals (`~/droneops-staging/drive-logs/`) | **584** | all parsed |
| …already present as `dji_txt` (hash overlap) | **20** | matches §8a exactly |
| **Distinct real DJI logs evaluated** | **762** | 198 + 584 − 20 |
| **Comparisons run** | **782** | the 20 overlapping logs were evaluated in both corpora |

So the figure for "every retained original" is **198**, not 182, 184, 190 or
192. The plan's instruction to re-derive was the correct guidance; the numbers
written beside it are a week stale, and will be stale again next week.

The 20 duplicates are a free determinism check: **0 of the 20 pairs disagreed on
the pinned `gps_track` digest**, so the harness is deterministic across
independent passes over the same bytes.

### 5.3 Coverage — everything decoded, nothing skipped

```
corpus               logs  both parsed  frames decoded  parse errors
prod                  198          198             198             0
recovered             576          576             576             0
nondji_header           8            8               8             0
TOTAL                 782          782             782             0

keychain provenance: {'fetched': 759, 'cache': 23}
logs with zero decoded frames: 0
log_version distribution: {14: 782}
frames/log: min=13 median=7947 max=34377 total=6,797,600
gps coordinates compared (total): 6,756,743
```

Every log is DJI log **v14** — i.e. every one is encrypted and required a
keychain. **759 keychains were fetched from DJI's API and 0 fetches failed**;
the 23 cache hits are the 3 pilot logs plus the 20 corpus overlaps. Total
wall-clock 1202 s on 5 cores.

This is the point at which the evaluation could have been worthless: the plan's
own §6 note and the task brief both warn against reporting results from logs
that failed to decode. **None failed.** `frames_decoded` is true for 782/782
and `point_count > 0` for all of them, so every headline metric in this report
is frame-derived, not a header fallback.

> **Correction to my own earlier reading, and a vindication of plan §8a.** I
> initially quarantined 8 of the 584 recovered files because their first four
> bytes are `00 00 00 00` rather than the `29 03 00 00` the other 576 carry, and
> I expected them to fail. **They all parse, on both crate versions, identically.**
> The leading `u32` is not a format marker, so my heuristic was wrong. §8a's
> claim that all 584 parse is correct. They are retained as a separate corpus row
> above only so the claim is visible rather than silently folded in.

### 5.4 The result

```
logs comparable          : 782
logs with ANY difference : 0

metric                      logs differing
duration_secs                            0
total_distance                           0
max_altitude                             0
max_speed                                0
home_lat                                 0
home_lon                                 0
point_count                              0
product_type                             0
frames_decoded                           0
frame_count                              0
gps_track_digest                         0      ← 6,756,743 coordinates
battery_sn                               0
drone_model                              0
aircraft_name                            0
aircraft_sn                              0
rc_sn                                    0
camera_sn                                0
log_version                              0
start_time                               0
header_duration_raw                      0
header_distance                          0
header_max_height                        0
header_max_hspeed                        0
dropped_segments                         0

NO FIELD DIFFERED on any log
```

**Unexplained differences: none, because there are no differences at all.** §6
step 5 asks every difference to be classified as an expected improvement or
unexplained; the set is empty. §6 step 6's adoption rule — "adopt only if every
headline metric is bit-identical" — is satisfied on the evidence. The reason not
to adopt is §0's: there is no release to adopt, and the candidate delivers
nothing.

`battery_sn` deserves a specific note, because it is the **only** field the
candidate commit can move: it is identical on all 782 logs. That is the expected
result, not a surprising one — see §6.

---

## 6. Why "zero differences" is a real finding and not a broken harness

A harness that reports "nothing differed" over 782 logs is indistinguishable
from a harness that cannot detect a difference. So the harness was falsified
deliberately, against the one behaviour the candidate actually changes.

`p-eval --selftest`:

```
── control 1: candidate differs from 0.5.7 behaviour on Inspire1 ──
  bytes              : [30, 39, 38, 37, 36, 35, 34, 33, 32, 31]
  0.5.7 (utf8 decode): "0987654321"
  candidate (BCD)    : "1234567890"
  PASS — a real behavioural difference exists and is observable
── control 2: the change is scoped to Inspire-1 product types ──
  Avata2         -> "0987654321" same as 0.5.7
  Matrice30      -> "0987654321" same as 0.5.7
  Mavic3Pro      -> "0987654321" same as 0.5.7
  Unknown(178)   -> "0987654321" same as 0.5.7
  Unknown(137)   -> "0987654321" same as 0.5.7
  Unknown(139)   -> "0987654321" same as 0.5.7
  Inspire1       -> "1234567890" DIFFERS (expected)
  Inspire1Pro    -> "1234567890" DIFFERS (expected)
  Inspire1RAW    -> "1234567890" DIFFERS (expected)
── control 3: diff_fields reports differences, not always [] ──
  identical inputs           -> []
  six perturbed fields       -> ["total_distance", "home_lat", "point_count", "product_type", "gps_track_digest", "battery_sn"]
  one-ULP max_altitude       -> ["max_altitude"]

selftest failures: 0
```

Control 1 proves the two linked crates are genuinely different code — if Cargo
had unified them into one package, the two answers would be equal and this would
fail. Control 2 proves the difference is reachable **only** through the three
Inspire-1 product types, so "zero differences over a corpus containing no
Inspire 1" is the *predicted* outcome. Control 3 proves the comparator reports
differences rather than always returning `[]`, down to one ULP on a float.

`cargo test` in the harness, which includes production's own `gate.rs` tests:

```
running 13 tests
test gate::tests::accepts_normal_segment_without_dt ... ok
test gate::tests::accepts_normal_segment_with_dt ... ok
test gate::tests::header_within_bound_is_trusted ... ok
test gate::tests::rejects_long_segment_without_dt ... ok
test gate::tests::corrupt_header_is_bounded_to_datetime_span ... ok
test gate::tests::rejects_teleport_with_dt ... ok
test gate::tests::zero_or_negative_dt_falls_back_to_distance_gate ... ok
test tests::candidate_changes_only_inspire1_battery_sn ... ok
test tests::every_compared_field_is_actually_compared ... ok
test tests::float_comparison_is_bitwise_not_tolerant ... ok
test tests::production_gate_is_the_one_included ... ok
test tests::identical_metrics_report_no_difference ... ok
test tests::provenance_field_is_not_compared ... ok

test result: ok. 13 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
```

`every_compared_field_is_actually_compared` perturbs all 24 fields one at a time
and asserts each is reported — without it, a field silently dropped from the
comparator would make this report's central table read `0` for the wrong reason.

`flight-parser`'s own suite, unchanged by this work:

```
cd flight-parser && cargo test
test result: ok. 65 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.07s
```

### 6.1 A second control: the harness against production's own stored rows

The harness transcribes `dji.rs`'s metric logic, so it could in principle be a
faithful comparison of two crates and still not be what production computes.
Checking the pinned arm against the 198 production `flights` rows for the same
file hashes partitions cleanly by **when the row was written**:

| Row era | metric | harness == prod DB |
|---|---|---:|
| Written by today's parser (`raw_metadata` carries `header_duration_raw` + `dropped_segments`) — **73 rows** | `duration_secs` | **73 / 73** |
| | `total_distance` | **73 / 73** |
| Legacy rows (those keys absent — imported before ADR-0027 / ADR-0028) — **125 rows** | `duration_secs` | 62 / 125 |
| | `total_distance` | 123 / 125 |
| All 198 rows | `max_altitude`, `max_speed`, `point_count`, `frame_count`, `product_type` | **198 / 198 each** |

**On every row production wrote with the current logic, the harness reproduces
production exactly.** Every disagreement is confined to rows that predate the
logic, and each has a named cause:

- **63 legacy `duration_secs`** — ADR-0027 replaced a frames/10 estimate with the
  header value. Example `0136ecbe7c`: stored **1409.5 s**, header **1423.4 s**.
  Deltas: median **+0.0 s**, max **+13.9 s**, only **6 rows** off by more than
  1 s, **+35.7 s** across all 125.
- **2 legacy `total_distance`** — ADR-0028's C1 outlier gate did not exist when
  they were written. The harness reports `dropped_segments = 1` for both and the
  stored row records none; the difference is the dropped teleport
  (`c1bb1d784e`: 9962.23 m vs 9970.03 m; `dff2de5ff4`: 1082.38 m vs 1091.82 m).
  **This is the gate working, not a defect.**
- **129 `drone_model`** — all 129 are rows with `aircraft_id IS NOT NULL`. The
  harness reports the raw header `aircraft_name` (`Matrice 4TD`); production
  stores the canonical fleet aircraft name that ADR-0044's matcher assigned
  (`DJI Matrice 4TD`). Expected, and evidence that attribution is working.

**Incidental finding for P3/P4 (not a P-EVAL deliverable).** 153 of the 226
`dji_txt` rows are legacy rows. D5 explicitly freezes duration and distance
("Duration / distance / max altitude / max speed stay untouched"), so those
rows keep their pre-ADR-0027/0028 values indefinitely unless something
reprocesses them. The error is small — 6 rows wrong by more than a second, 35.7 s
of airtime in total across the 125 measured — and leaving it alone is a
defensible call. It should be a *decision*, not an oversight. Flagging it here;
it is Bill's call, not mine.

---

## 7. Reproducing this

```bash
# Build (matches flight-parser/Dockerfile's pinned toolchain)
docker run --rm -v "$PWD":/repo -v /some/target:/target \
  -e CARGO_TARGET_DIR=/target -w /repo/tools/p-eval \
  rust:1.85-bookworm cargo build --release

# Falsification controls — no logs, no key, no network
/some/target/release/p-eval --selftest

# The comparison. DJI_API_KEY is the value from system_settings.dji_api_key,
# trimmed (production does `.strip()`); it is read from the environment and
# never written to the cache, the results file, or stdout.
p-eval --corpus prod=/logs/prod --corpus recovered=/logs/recovered \
       --cache /cache --out /out/full.jsonl
```

Keychains are cached by log hash, so a rerun costs **zero** DJI API calls and
hands both crate versions a byte-identical keychain. The cache contains log
decryption material and lives outside the repository; it is not committed.

---

## 8. Follow-ups this evaluation surfaced

### 8.1 `flight-parser/Cargo.toml` does not pin the crate — the lockfile is the only gate

```toml
dji-log-parser = "0.5"     # = ^0.5.0
```

D6's whole point is that this crate moves only after Bill has seen a before/after
diff. A caret range does not express that. It is inert **today** only because
`0.5.7` is the maximum of `^0.5`; the moment upstream publishes `0.5.8`, any
`cargo update`, any lockfile-less build, or any dependency refresh adopts it
silently and D6 is bypassed without anyone deciding anything.

**Recommendation:** pin `dji-log-parser = "=0.5.7"`. One line, no behaviour
change today, and it makes D6 mechanical instead of a matter of remembering.
Deliberately **not** done here — the brief for this phase is evidence only, and
it is a change to the manifest D6 governs.

### 8.2 `DJI_LOG_PARSER_VERSION` is a hand-maintained string with nothing tying it to the lockfile

```rust
// flight-parser/src/dji.rs:7
pub const DJI_LOG_PARSER_VERSION: &str = "0.5.7";
```

It is stamped onto **every** `flight_details` row as `crate_version`, and D6's
stated payoff is that *"re-backfill everything decoded below version X"* becomes
a query rather than archaeology. Nothing checks it against `Cargo.lock`, and a
second hardcoded `"0.5.7"` sits in a test fixture at `details.rs:1840`. Bump the
dependency and forget the constant, and that query silently lies about every row
written afterwards — the provenance is wrong in exactly the situation it exists
to handle.

**Recommendation:** a test that parses `Cargo.lock` and asserts the constant
matches the resolved `dji-log-parser` version. Cheap, and it fails loudly at the
moment of the mistake.

### 8.3 Upstream is dormant, so `Unknown(NNN)` is permanent

One commit in 15 months, last release 2025-04-26, 20 open issues. Meanwhile the
corpus contains **four** unresolved product types — `Unknown(137)` Mavic 4 Pro,
`Unknown(139)` Mini 5 Pro, **`Unknown(150)` Matrice 4T**, `Unknown(178)`
Matrice 4TD — and every airframe BarnardHQ buys next will add another.

Note the plan and the prod DB only surface three: `Unknown(150)` appears on 39
recovered ODL-era logs and on **no** native `dji_txt` row, so it becomes live the
moment P7 re-imports. P4(b)'s `^Unknown\(\d+\)$` repair predicate already covers
it by construction — worth confirming the P7 dry-run expects 150 as a fourth
value rather than the three the plan names.

The consequence: `dji.rs`'s `Unknown(NNN) → aircraft_name` fallback is permanent
infrastructure. It should be documented as the airframe-naming mechanism, not as
a workaround awaiting an upstream fix that the evidence says is not coming.

### 8.4 A fresh dependency resolve no longer builds on the pinned toolchain

Resolving `dji-log-parser`'s transitive graph from scratch on `rust:1.85`
(the version `flight-parser/Dockerfile` pins) **fails**:

```
error: rustc 1.85.1 is not supported by the following packages:
  icu_collections@2.3.0 requires rustc 1.88
  icu_locale_core@2.3.0 requires rustc 1.88
  icu_normalizer@2.3.0 requires rustc 1.88
  icu_properties@2.3.0 requires rustc 1.88
  icu_provider@2.3.1 requires rustc 1.88
  idna_adapter@1.2.2 requires rustc 1.86
```

`flight-parser` builds only because **`Cargo.lock` is committed** and holds the
older `idna 1.1.0` / `idna_adapter 1.2.1` / `icu_* 2.1.x` chain (reached through
`ureq` → `url` → `idna`). This harness's lockfile pins the same versions
deliberately, so it builds on the same toolchain and runs on production's
dependency graph.

**Practical rule:** never run `cargo update` in `flight-parser` without bumping
`rust:1.85-bookworm` in its `Dockerfile` in the same change. Today that would
turn a routine refresh into a build failure; the committed lockfile is
load-bearing, not incidental.

---

## 9. What I could not verify

- **The `SmartBatteryStatic` shim's behaviour on a real log.** §6's scope split
  assigns the pack-value comparison to P2's records spike, so §4's analysis is
  read from the crate source and from the census's recorded values — **not
  measured**. The `loop_times ≤ 255` limit is derived arithmetically from the
  struct layout and is labelled as such. It should be confirmed in P2-a against a
  pack with a three-digit cycle count.
- **Peak RSS against the parser's 256 MB `mem_limit`.** Still unmeasured, and
  still P2-a's gate. The harness is not a proxy: it holds two full frame vectors
  and two track copies, so its footprint is structurally larger than the
  parser's. I deliberately did not report a number that would be mistaken for one.
- **Whether a newer crate would help**, in any sense beyond what is published.
  There is no 0.5.8 and no 0.6.x to test. If upstream revives, this harness is
  the instrument: change one `rev`/`version` in `tools/p-eval/Cargo.toml` and
  rerun against the cached keychains at no API cost.
- **Unknown(139) and Unknown(150) identity.** Taken from plan §8a (Mini 5 Pro,
  Matrice 4T) — the logs carry only a user-chosen `aircraft_name`
  (`BigThingsSmallPackages`, `MATRICE 4T`), so the mapping from 139/150 to a
  model name is inherited, not independently established here.
