# Flight Details — pull the untapped DJI log data into the DB and give it a page

**Status:** PLANNED, nothing built. Implementation plan for ROADMAP **FP-1**.
**Date:** 2026-09-04 (revised same day to fold in operator decisions)
**Decision record:** [ADR-0043](../adr/0043-flight-details-sidecar-table-for-extended-log-data.md)
**Input:** [`2026-09-04-dji-log-untapped-data-census.md`](2026-09-04-dji-log-untapped-data-census.md)
— the primary-source census of what the logs actually carry. This plan does not
re-derive it; it consumes it.

## Operator ask (Bill, 2026-09-04, verbatim)

> "we should think about a plan to make all this extra data more accessible and
> to make sure to pull it into the DB - even if that is just a 'Flight Details'
> link somewhere on a flight in the 'flights' menu - then later we can figure
> out where else to pull that data in to utilize it."

Scope: capture breadth into the database durably, expose one Flight Details view
off the Flights menu, leave later utilisation (reports, battery, maintenance)
open. **Breadth of capture beats depth of presentation.**

## Operator decisions (Bill, 2026-09-04) — DECIDED, not open

The first draft of this plan carried seven open questions. All are now answered.
They are recorded here because several of them **enlarge** the scope
significantly — in particular D2 (full-resolution series) forces a storage
redesign, and D5/D7 mean the backfill now deliberately writes to `flights`,
which the first draft forbade outright.

| # | Decision | Effect on this plan |
|---|---|---|
| **D1** | **Store the full raw pilot position track** (AppGPS lat/lon), plus derived pilot-to-aircraft distance. Reports must not include it unless deliberately added. | §2.3, §2.5, §4.4 (report-audience guard made explicit and testable). |
| **D2** | **Full resolution at rest.** No downsampling when stored; downsample only at the API/read layer. | Forces series out of the sidecar into a dedicated `flight_series` table — §1.5, the plan's biggest structural change. |
| **D3** | **Flight Details link on EVERY flight**, any source. Sections with no data render "not available for this source." | §4.1, §5. The `source === 'dji_txt'` gate is dropped. |
| **D4** | **Logs become the source of truth for batteries.** Un-gate P6: write `battery_logs.discharge_mah`, drive `batteries.cycle_count` / `health_pct` from the pack's own values, keep the hand-entered numbers in history but stop showing them as current. | §1.6, §6 P6. |
| **D5** | **Backfill also repairs existing rows:** re-stamp `gps_track` / `telemetry` timestamps on the 210 DJI flights, and replace the literal `drone_model = "Unknown(NNN)"` with the header `aircraft_name`. Duration / distance / max altitude / max speed stay untouched. | §3.4 — a *new* guarantee mechanism is required, because the original one (never load the `Flight` entity) no longer applies. |
| **D6** | **Evaluate the library bump first**, with a before/after diff over all retained logs. Adopt only if nothing moves unexplained. Output a diff report in `docs/`. | New phase **P-EVAL**, §6. |
| **D7** | **Re-import OpenDroneLog-era flights** if original DJI files are recovered, replacing the matched `opendronelog_import` rows and carrying over mission attachments, notes, tags, pilot, aircraft. Unmatched files import as new. Needs a matching rule and a dry-run. | New phase **P7**, §7. Blocked on the log inventory (§8). |

---

## 1. Data model

### 1.1 Options considered

**(a) Widen `flights.telemetry` + add a `flights.flight_details` JSON column.**

- **Pro:** no new table, no join; the existing `/telemetry` read path already
  serves series.
- **Con — decisive:** `flights` is the subject of three OOM ADRs (ADR-0019 list
  defer, ADR-0020 report geo buffer, ADR-0025 mission detail). It already
  carries three heavy JSON columns (`models/flight.py:47-49`). Adding more makes
  every `select(Flight)` heavier, protected only by remembering to `defer()` —
  a discipline that has already failed in production. The live example:
  `reprocess_all_from_stored` does a bare `select(Flight).where(...)` with no
  defer (`flight_library.py:1650-1659`).
- **Con:** `Flight.telemetry` is SQLAlchemy generic `JSON` → Postgres `json`,
  not `jsonb`. Nothing stored there can ever be GIN-indexed without an
  `ALTER TABLE … USING` rewrite of a large TOASTed column.
- **Fatal under D2.** At full resolution the series alone are ~1.3 MB of raw
  JSON per flight (§1.5). Putting that on `flights` makes every full-entity load
  in the codebase a multi-megabyte detoast.

**(b) A `flight_details` sidecar table, 1:1 with `flights`.**

- **Pro:** extended data becomes opt-in **by construction** — the list, mission,
  report and map paths never join it, so they cannot be hurt by it.
- **Pro:** typed scalars are directly filterable/aggregatable later with plain
  b-tree indexes; `JSONB` groups keep GIN indexing available for `events`.
- **Pro:** the details half of the backfill can select `(Flight.id,
  Flight.source_file_hash)` as *columns* and never load the `Flight` entity.
- **Con under D2:** a ~300 KB compressed `series` column on this table
  re-creates the ADR-0019 trap one level down — any `select(FlightDetails)`
  entity load detoasts the whole thing to read one scalar.

**(c) Fully normalized child tables, down to per-sample rows.**

- **Pro:** real SQL over every value.
- **Con — decisive at sample granularity:** the M4TD census log has 13,870
  frames; one series across 210 flights is ~2.9 M rows, and 14 series is ~41 M
  rows. There is no read path that needs per-sample SQL, and the write cost on a
  streaming-replicated primary is real.

### 1.2 Recommendation — (b) **split**, with series in their own table

**Scalars, groups and summaries → `flight_details` (1:1, PK = `flight_id`).
Time series → `flight_series` (one row per flight *per named series*).**

This is a change from the first draft, forced by D2. The reasoning is in §1.5;
the short version is that a per-series row lets a chart request detoast ~110 KB
instead of ~1.3 MB, and keeps the scalar row small enough that a careless
full-entity load is harmless.

### 1.3 `flight_details` — scalars and groups

Units follow ADR-0032 conventions throughout: **metres, m/s, volts, amps, °C,
degrees, mAh, Wh, seconds, Hz**, with the unit in the column name. All columns
except `flight_id`, `schema_version`, `generated_at` are **nullable** — a
Litchi/Airdata flight has no row at all, and a log whose frames failed to decode
produces a mostly-NULL row.

```
flight_details
  flight_id                     UUID       PK, FK flights.id ON DELETE CASCADE
  schema_version                SMALLINT   NOT NULL DEFAULT 1
  parser_version                VARCHAR(32)
  crate_version                 VARCHAR(32)          -- dji-log-parser version used (D6)
  generated_at                  TIMESTAMP  NOT NULL  -- naive UTC, repo convention

  -- decode provenance
  frame_count                   INTEGER
  record_count                  INTEGER
  frame_hz_est                  DOUBLE PRECISION
  first_frame_at                TIMESTAMP
  last_frame_at                 TIMESTAMP

  -- altitude (MSL/VPS; AGL stays on flights.max_altitude, untouched)
  max_altitude_msl_m            DOUBLE PRECISION
  min_altitude_msl_m            DOUBLE PRECISION
  home_altitude_msl_m           DOUBLE PRECISION
  max_vps_height_m              DOUBLE PRECISION
  take_off_altitude_raw         DOUBLE PRECISION     -- units unconfirmed, see §9 C-1
  take_off_altitude_units       VARCHAR(16)          -- 'unconfirmed' until C-1 closes

  -- range / rates
  max_distance_from_home_m      DOUBLE PRECISION
  max_climb_rate_ms             DOUBLE PRECISION     -- positive = climbing
  max_descent_rate_ms           DOUBLE PRECISION     -- positive magnitude, descending
  header_max_vertical_speed_ms  DOUBLE PRECISION

  -- phases
  takeoff_count                 SMALLINT
  landing_count                 SMALLINT
  rth_count                     SMALLINT
  sport_mode_seconds            DOUBLE PRECISION
  waypoint_mode_seconds         DOUBLE PRECISION
  manual_mode_seconds           DOUBLE PRECISION

  -- camera
  photo_count                   INTEGER              -- camera.is_photo RISING edges
  header_capture_num            INTEGER
  video_seconds                 DOUBLE PRECISION
  header_video_time_s           DOUBLE PRECISION

  -- RC link (raw crate scale; 0-90 observed, 104 seen once — NOT rescaled)
  rc_downlink_min               SMALLINT
  rc_downlink_avg               DOUBLE PRECISION
  rc_downlink_max               SMALLINT
  rc_uplink_min                 SMALLINT
  rc_uplink_avg                 DOUBLE PRECISION
  rc_uplink_max                 SMALLINT
  rc_zero_downlink_frames       INTEGER
  rc_disconnect_events          SMALLINT
  ofdm_signal_avg_pct           DOUBLE PRECISION

  -- battery, this flight (from frames, full resolution)
  battery_current_max_a         DOUBLE PRECISION
  battery_energy_wh             DOUBLE PRECISION     -- ∫ V·I dt
  battery_discharge_mah         DOUBLE PRECISION     -- ∫ I dt
  battery_cell_count            SMALLINT
  battery_cell_deviation_max_v  DOUBLE PRECISION
  battery_temp_min_c            DOUBLE PRECISION
  battery_temp_max_c            DOUBLE PRECISION
  battery_full_capacity_mah     DOUBLE PRECISION
  battery_current_capacity_mah  DOUBLE PRECISION

  -- pack lifetime (Tier 1 SmartBatteryStatic, shim-corrected — §2.4)
  pack_cycle_count              INTEGER
  pack_designed_capacity_mah    INTEGER
  pack_full_charge_voltage_v    DOUBLE PRECISION
  pack_values_shimmed           BOOLEAN
  pack_values_plausible         BOOLEAN              -- false → never displayed, never used for D4

  -- config in force for this flight
  height_limit_m                DOUBLE PRECISION
  go_home_height_m              DOUBLE PRECISION
  max_allowed_height_m          DOUBLE PRECISION
  is_beginner_mode              BOOLEAN

  -- identity
  aircraft_sn_full              VARCHAR(32)          -- 20-char, vs header's 16
  app_platform                  VARCHAR(32)

  -- pilot / VLOS (D1: raw track lives in flight_series; these are the rollups)
  pilot_sample_count            INTEGER
  pilot_max_distance_m          DOUBLE PRECISION
  pilot_avg_distance_m          DOUBLE PRECISION
  pilot_track_stored            BOOLEAN              -- true when raw lat/lon series exist

  -- rollups so the UI can badge without opening a JSONB
  event_count                   INTEGER
  warning_event_count           INTEGER
  anomaly_flag_count            SMALLINT

  -- repair provenance (D5 / D7) — keeps flights.raw_metadata clean
  gps_timestamps_restamped_at   TIMESTAMP            -- D5(a)
  drone_model_previous          VARCHAR(255)         -- D5(b), the "Unknown(NNN)" literal
  replaced_from                 JSONB                -- D7: prior source + prior metrics

  -- JSONB groups (small, structured, potentially searchable)
  phases    JSONB   -- [{state, seconds, frames}]
  events    JSONB   -- [{t_offset_s, last_t_offset_s, kind, severity, message, count, garbled}]
  config    JSONB   -- failsafe_action, go_home_mode, obstacle_avoidance, mvo, long tail
  firmware  JSONB   -- [{component, version}]
  health    JSONB   -- {is_vibrating: N frames, is_compass_error: N, ...}
  sd_card   JSONB   -- {min_remain_capacity_mb, min_remain_photo_num, min_remain_video_s}
  serials   JSONB   -- {rc_sn, camera_sn, battery_sn, component_serials[]}
  extra     JSONB   -- forward-compat catch-all; normally NULL
```

No `series` column. That is the D2 change.

**Indexes at P0: the primary key only.** No secondary index ships until a query
needs one. `JSONB` on the group columns is what keeps a later GIN on `events`
a one-liner.

**Relationship declaration is load-bearing:**

```python
# backend/app/models/flight.py
details = relationship("FlightDetails", back_populates="flight",
                       uselist=False, lazy="noload", cascade="all, delete-orphan")
series  = relationship("FlightSeries", back_populates="flight",
                       lazy="noload", cascade="all, delete-orphan")
```

`lazy="noload"` matches the existing `battery_logs` relationship
(`flight.py:58`). `selectin` on either would reproduce ADR-0019. A P0 test
asserts the compiled list query references neither table.

### 1.4 `flight_series` — the time series

```
flight_series
  flight_id     UUID        NOT NULL, FK flights.id ON DELETE CASCADE
  source        VARCHAR(16) NOT NULL   -- 'frame' | 'pilot' | 'ofdm' | 'camera'
  name          VARCHAR(48) NOT NULL   -- 't_offset_s', 'altitude_msl_m', 'pilot_lat', ...
  unit          VARCHAR(16)            -- 'm', 'm/s', 'deg', 'V', 'A', 'pct', 's', 'wgs84'
  sample_count  INTEGER     NOT NULL
  precision_dp  SMALLINT               -- decimal places emitted (§2.5 rounding)
  values        JSON        NOT NULL   -- numeric array, FULL resolution — see §1.5
  PRIMARY KEY (flight_id, source, name)
```

The composite PK gives the exact lookup the read path wants
(`WHERE flight_id = ? AND source = 'frame' AND name = ANY(?)`) with no extra
index. `source` exists because **the series do not share one time base** —
frames, AppGPS and OFDM are recorded at different cadences (13,870 / 657 / 5,538
records respectively in the M4TD census log). Each `source` group carries its own
`t_offset_s` row, and every series within a group is index-aligned to it. This is
a detail the first draft got wrong by assuming a single decimated time base.

### 1.5 Why series moved out, and what `values` should be — the D2 analysis

**Size, recomputed from the census.** Largest observed log: M4TD, 13,870 frames.

| Block | Rows × samples | Bytes/value (rounded, §2.5) | Raw JSON |
|---|---|---|---|
| Frame series (14) | 14 × 13,870 | ~6 | ~1.11 MB |
| Frame `t_offset_s` | 13,870 | ~7 | ~97 KB |
| Pilot (lat/lon/dist/t) | 4 × 657 | ~10 avg (13 for coords) | ~26 KB |
| OFDM (signal + t) | 2 × 5,538 | ~5 | ~55 KB |
| **Total per flight** | | | **≈ 1.3 MB raw** |

TOAST-compressed (pglz over rounded ASCII numeric runs, typically 3–5×):
**≈ 260–430 KB for the largest flight**, and roughly 100–150 KB for a typical
5,000-frame flight. Across 210 flights: **≈ 25–60 MB one-time**, plus ~100–300 KB
per new flight.

*Note on the operator's figure:* the "0.5–2 MB/flight compressed" estimate is
close to the **raw** number, not the compressed one — compression is the
difference. The scale conclusion is unchanged either way: this is real but not
alarming. For calibration, ADR-0019 measured `gps_track + telemetry +
raw_metadata` at **33–44 MB compressed across the top 500 flights**, so the new
series are roughly 0.6–1.8× the existing heavy-JSON footprint. Postgres handles
this comfortably; the risk is entirely in *read paths*, which is exactly why the
storage shape matters.

**Why not keep it in `flight_details`.** A TOASTed column is only detoasted when
that column is selected — so co-location is not fatal *provided every reader is
column-explicit*. But `select(FlightDetails)` (a normal entity load, which is
what ORM code writes by default) selects all columns and would detoast ~300 KB to
read one scalar. That is ADR-0019's exact failure mode, one level down, and
relying on every future caller to be column-explicit is the same discipline that
already failed once. Splitting removes the possibility.

**Why one row per series, not one blob row.** The Flight Details page and any
future chart asks for 1–4 named series. Under a single-blob design, serving one
2,000-point chart detoasts, decompresses and parses the full ~1.3 MB. Under
one-row-per-series it touches ~110 KB. That is a ~12× difference on the hot path,
on a backend with a 1536 MiB cgroup and a documented history of OOM incidents. It
costs nothing structurally.

**Why `json` and not `jsonb` for `values` — deliberately the opposite of the
group columns.** `jsonb` is a parsed binary format: it re-encodes every number as
a variable-length `numeric` and adds a per-element `JEntry` header. For a
13,870-element float array that is tens of KB of pure overhead before the values,
and the binary numerics compress worse than ASCII digit runs. The only thing
`jsonb` buys is containment/path operators and GIN indexing — worthless for an
opaque float array we always fetch whole. So: **`json` for `flight_series.values`,
`jsonb` for `flight_details`' group columns.** The distinction is intentional and
should not be "tidied up" later.

**Alternative considered — `DOUBLE PRECISION[]`.** A native float8 array is 8
bytes/element fixed, needs no JSON parse in Python, and maps straight to a list.
Against it: pglz compresses high-entropy IEEE mantissas poorly, whereas rounded
ASCII (`193.9`) compresses well — so the text form is likely *smaller on disk*
despite being larger uncompressed. That is a claim I have not measured.

**P0 acceptance gate (measurement, not a guess):** before P1 locks the encoding
in, write one real flight's series both ways into a scratch table on
`droneops-standby-db` and compare `pg_column_size()` and read latency. Nothing
has been written at that point, so flipping `values` to `float8[]` is a
one-line migration change. Record the numbers in `PROGRESS.md`. **Default is
`json`; the measurement can overturn it.**

### 1.6 Battery model changes (D4)

```
batteries
  + cycle_count_observed  INTEGER          -- preserved hand/incremental history
  + metrics_source        VARCHAR(16)      -- 'observed' | 'pack'

battery_logs
  + pack_cycle_count      INTEGER          -- the pack's own count at that flight
```

`batteries.cycle_count` (`models/battery.py:21`) is today a counter incremented
once per imported flight (`flight_library.py:2568`, `battery.cycle_count += 1`) —
it counts *flights we have logs for*, not the pack's lifetime cycles. The pack
reports its own: 23 on the M4TD, 9 on the M30.

Semantics after D4:

- `cycle_count` becomes the **displayed current** value. When a plausible
  `pack_cycle_count` exists for the serial, it is pack-derived and
  `metrics_source = 'pack'`; otherwise the legacy increment continues and
  `metrics_source = 'observed'`.
- The pre-existing value is moved into `cycle_count_observed` **once** (only when
  it is NULL), and the per-flight increment continues to maintain
  `cycle_count_observed` forever, so the two stay comparable. History is kept;
  it is just no longer what the UI shows.
- **Monotonic guard:** cycles only ever increase on a pack, so pack-sourced
  writes take `GREATEST(cycle_count, pack_cycle_count)`. Without this, a backfill
  that processes flights in arbitrary order would leave the count at whatever the
  last-processed (possibly oldest) flight reported.
- `health_pct` is derived as
  `battery_full_capacity_mah / pack_designed_capacity_mah × 100`, written only
  when both are plausible and the ratio lands in 40–105 %. Outside that band it
  is left NULL rather than written wrong.
- `battery_logs.discharge_mah` (`models/battery.py:52`, always NULL today) is
  filled from `flight_details.battery_discharge_mah`.
- `battery_logs.cycles_at_time` keeps its existing meaning (the observed
  counter). The pack's value goes in the **new** `pack_cycle_count` column —
  silently redefining an existing column mid-history would corrupt the series it
  already holds.
- `pack_values_plausible = false` rows are never used as a source for any of this.

### 1.7 Migrations

- `0010_flight_details` — creates `flight_details` **and** `flight_series`.
  `down_revision = "0009_mission_dl_email_sent_at"`.
- `0011_battery_source_of_truth` — the three additive nullable columns in §1.6.

Both **must be idempotent** (ADR-0042, the 2026-08-22 fresh-install incident):
`0001_baseline_schema` builds fresh databases with `create_all` from the **live
models**, so on a fresh DB these objects already exist when the revision runs.
Guard exactly as `0009` does (`0009_mission_dl_email_sent_at.py:37-40`):

```python
conn = op.get_bind()
insp = sa.inspect(conn)
if "flight_details" in insp.get_table_names():
    return  # fresh DB: 0001's live-models create_all already built it
```

Revision ids are ≤ 32 chars (the constraint 0007/0009 call out). Additive DDL, no
backfill inside the migration — replication-safe over WAL, survives container
recreation and standby promotion, no port / `pg_hba` / connection-string change.

---

## 2. Parser contract (`flight-parser` → backend JSON)

### 2.1 Shape

One new optional field on `ParsedFlight` (`main.rs:19-40`):

```rust
#[serde(skip_serializing_if = "Option::is_none")]
pub details: Option<FlightDetails>,
```

`FlightDetails` carries the §1.3 scalars, the JSONB groups, and
`series: Vec<SeriesBlock>` where each block is
`{source, name, unit, precision_dp, values}` — mapping 1:1 onto `flight_series`
rows.

Litchi (`litchi.rs:169`) and Airdata (`airdata.rs:208`) build `ParsedFlight`
struct literals, so the new field is a **compile error** in both until
`details: None` is added — the compiler enforces the audit. Their JSON output is
byte-identical afterwards. The backend treats absent/`null` as "no details, no
series."

### 2.2 Tier 0 — from `log.frames()`

The parser already iterates every frame (`dji.rs:139-207`); these are additional
accumulators in that same loop. No second decode.

| Group | Frame source | Output |
|---|---|---|
| Time base | `frame.custom.date_time` | `series[frame].t_offset_s`, `first/last_frame_at`, `frame_hz_est`. Also fills the currently-empty `TelemetryData.timestamps` (`dji.rs:120`) and `TrackPoint.timestamp`. |
| Link quality | `rc.downlink_signal`, `rc.uplink_signal` | `rc_*` scalars, `series[frame].rc_downlink/rc_uplink`. Also fills `TelemetryData.signal_strength` (hard-coded `None` at `dji.rs:265` — why `/telemetry` has always served `signal_strength: null`). |
| Distance from home | `home.latitude/longitude` vs `osd` | `max_distance_from_home_m`, `series[frame].distance_from_home_m`, and `TelemetryData.distance_from_home` (`dji.rs:266`). |
| Phase timeline | `osd.flyc_state`, `osd.flight_action`, `osd.flyc_command` | `phases`, `takeoff_count`, `landing_count`, `rth_count`, `*_mode_seconds`; transitions emitted as `events` of kind `mode`. |
| Camera | `camera.is_photo` rising edges, `is_video`, `sd_card_state` | `photo_count`, `video_seconds`, `sd_card`. |
| Gimbal | `gimbal.pitch/roll/yaw`, `is_stuck`, `*_at_limit` | `series[frame].gimbal_*_deg`; `is_stuck` frames into `health`. |
| Attitude / vertical | `osd.pitch/roll/yaw`, `osd.z_speed` | `max_climb_rate_ms`, `max_descent_rate_ms`, `series[frame].z_speed_ms`, `aircraft_*_deg`. |
| MSL / VPS | `osd.altitude`, `osd.vps_height`, `home.altitude` | `max/min_altitude_msl_m`, `home_altitude_msl_m`, `max_vps_height_m`, `series[frame].altitude_msl_m`, `vps_height_m`. |
| Battery detail | `battery.current`, `current/full_capacity`, `cell_voltages[]`, `cell_voltage_deviation`, `min/max_temperature` | `battery_*` scalars, `series[frame].battery_current_a`, `cell_voltage_deviation_v`. |
| Safety flags | `is_vibrating`, `is_compass_error`, `is_motor_blocked`, `voltage_warning`, … | `health` frame-counts, `anomaly_flag_count`. |
| Config | `home.height_limit`, `go_home_height`, `max_allowed_height`, `is_beginner_mode`, `go_home_mode` | corresponding columns + `config`. |
| Events | `app.tip`, `app.warn` | `events` (§2.6). |
| Header extras | `max_vertical_speed`, `capture_num`, `video_time`, `take_off_altitude`, `app_platform` | corresponding columns; `take_off_altitude` stored **raw**, marked `unconfirmed`. |

**Also in P1 (supports D5b going forward):** when `details.product_type` renders
as `Unknown(NNN)`, fall back to the header `aircraft_name` for `drone_model`
(`dji.rs:27-31`) instead of emitting the literal. Without this, new Matrice 4TD /
Mavic 4 Pro imports keep producing `Unknown(178)` even after the backfill repairs
the existing 150 rows.

### 2.3 Tier 1 — from the raw record stream (**the plan's largest unknown**)

`dji.rs:104` calls `log.frames(keychains)` and nothing else, so `AppGPS`,
`SmartBatteryStatic`, `Firmware`, `Camera`, `MCParams`, `OFDM` and
`ComponentSerial` are unreachable today.

**Unverified, and it must be verified first:** the exact `dji-log-parser`
record-access API — signature, whether it consumes the keychains (which would
force a second DJI keychain round-trip per flight), return type, and peak memory.
The census says "one `records()` call away"; that is a reasonable read of the
crate source but was never exercised. **P2 opens with a spike that answers
exactly this** (§6). If the answer is bad, P2 stops and is re-planned; P0/P1/P3
already deliver most of the value.

| Record | → |
|---|---|
| `AppGPS` | **D1:** `series[pilot].pilot_lat`, `pilot_lon`, `pilot_distance_m`, `t_offset_s` (full raw track), plus `pilot_*` rollup scalars and `pilot_track_stored = true`. Absent from Goggles-generated logs (Avata 2, FPV) → NULL, not zero. |
| `SmartBatteryStatic` | `pack_cycle_count`, `pack_designed_capacity_mah`, `pack_full_charge_voltage_v` via the §2.4 shim. |
| `Firmware` | `firmware[]`. |
| `Camera` | `sd_card`, plus a `record_time` cross-check against frame-derived `video_seconds`. |
| `MCParams` | `config.failsafe_action`, `obstacle_avoidance`, `mvo`. |
| `OFDM` | `ofdm_signal_avg_pct`, `series[ofdm].signal_pct` + its own `t_offset_s`. |
| `ComponentSerial` | `aircraft_sn_full`, `serials.component_serials[]`. |
| `RC` / `RCDisplayField` | **Deferred.** Stick-position → pilot-input-intensity is a modelling exercise with no consumer; 4,147 samples/flight to display nothing. |

Tier 2 (`Unknown` record types, ~30 % of records in newer logs) stays out of
scope — unchanged from the census.

### 2.4 The `SmartBatteryStatic` shim — corrected characterization

The census calls these "big-endian-wrong." Working its own four recorded values,
the transform is uniformly **`raw >> 8`**, not a byte swap:

| Field | Raw | Hex | `>> 8` | Expected (pack spec) |
|---|---|---|---|---|
| `loop_times` (M4TD) | 5888 | `0x1700` | 23 | 23 |
| `loop_times` (M30) | 2304 | `0x0900` | 9 | 9 |
| `designed_capacity` (M4TD) | 1899520 | `0x1CFC00` | 7420 | 7420 mAh |
| `designed_capacity` (M30/TB30) | 1505282 | `0x16F802` | 5880 | 5880 mAh |

For a `u16` the two are indistinguishable. For the 32-bit `designed_capacity`
they are not, and the M30 low byte is `0x02` — **nonzero**, which a clean endian
swap of a well-formed field would not leave. That reads as a **field-offset /
alignment bug in the crate's struct decode**, not endianness. The distinction
matters: an alignment bug is more likely already fixed upstream, and more likely
to differ per record layout.

**Implementation:** `raw >> 8` (not `swap_bytes()`), behind a plausibility gate —
cycles `0..=3000`, designed capacity `1000..=30000` mAh, full-charge voltage
`10.0..=60.0` V. Out of range → store the value but set
`pack_values_plausible = false`; it is then never displayed and never feeds D4.
`pack_values_shimmed` records that the correction was applied, so shimmed rows
stay distinguishable if upstream fixes it.

**P-EVAL (§6, D6) checked whether the shim is needed at all. It is.** Upstream
has not fixed this — `record/smart_battery_group.rs` is byte-for-byte identical
between `0.5.7` and upstream `master`. Two corrections from the 2026-09-11 report:
the hex for 1505282 is `0x16F802` (fixed in the table above — `0x16F002` is a
different number), and **`raw >> 8` recovers the true value only while its
most-significant byte is zero.** That is always true for `designed_capacity` and
`full_charge_voltage`, but **`loop_times` is a `u16`, so the shim wraps at 256
cycles** — a pack at a true 260 cycles reads as 4, which passes the `0..=3000`
plausibility gate and is stored as a plausible value. P2-a must either read the
field at the corrected offset or treat a *decreasing* observed cycle count as
non-authoritative (which is what §1.6's `GREATEST` monotonic guard already does —
worth keeping for that reason, not by coincidence).

### 2.5 Full resolution, and the rounding that pays for it (D2)

Two rules, and the order matters:

1. **Every scalar is computed at full frame resolution** — maxima, minima, edge
   counts and the `∫V·I dt` / `∫I dt` integrals never see a reduced array.
2. **Every series is stored at full resolution — one value per source sample.**
   No decimation at rest. Decimation happens only in the API layer (§4.2).

**Rounding is not downsampling.** Each series is emitted at physically meaningful
precision, recorded in `flight_series.precision_dp`:

| Quantity | dp | Rationale |
|---|---|---|
| altitude, distance (m) | 1 | 0.1 m is below any DJI sensor's real accuracy |
| speed, climb rate (m/s) | 2 | |
| angles (deg) | 1 | |
| voltage (V) | 3 | mV resolution matches the crate's own `/1000.0` |
| current (A) | 2 | |
| signal, percent | 0 | integers as reported |
| `t_offset_s` | 2 | 10 ms — finer than any observed cadence |
| lat / lon | 7 | ~11 mm; beyond GPS accuracy |

`193.90000000000001` → `193.9` is a ~4× reduction in stored text with **zero**
information loss. Every sample is kept; only spurious f64 mantissa digits are
dropped. This is what makes full resolution affordable (§1.5).

### 2.6 Event extraction and the garbled-string rule

The census found ~20 % of `app.tip` / `app.warn` strings arrive with a **garbled
prefix, intact suffix**, from the crate's decrypt/append path.

1. **Trim** leading bytes that are not printable ASCII or valid UTF-8, and any
   leading run before the first capital letter beginning a word-boundary run of
   ≥ 3 ASCII letters. Set `garbled: true` when anything was trimmed.
2. **Dedupe on the cleaned string.** This turns the M4TD's 18 separate "Remote
   controller disconnected. Adjust antennas" strings into **one** record with
   `count: 18`, `t_offset_s` = first, `last_t_offset_s` = last. Without it,
   `events` is a spam log; with it, it is a report section.
3. **Never reconstruct** a message from a partial. Under 8 characters after
   cleaning → `kind: "unparsed"` with the remnant and `garbled: true`. ADR-0028's
   posture: an unknown stays unknown.
4. Severity from source: `app.tip` → `info`, `app.warn` → `warning`,
   `AppSeriousWarn` → `serious`, mode transitions → `info`/`mode`.

Raw strings are not stored — cleaned message plus the `garbled` flag only.

### 2.7 Backward compatibility

- Litchi / Airdata: `details: None`, byte-identical output, compiler-enforced.
- Existing DJI field **values** unchanged. The only in-place changes are three
  always-empty fields getting populated — `TelemetryData.timestamps`
  (`dji.rs:120`), `signal_strength` (`dji.rs:265`), `distance_from_home`
  (`dji.rs:266`) — all three already served by `/telemetry`
  (`flight_library.py:2007-2015`) and already typed permissively on the frontend
  (`FlightReplay.tsx:64`). They go from `null` to arrays.
- `duration_secs`, `total_distance`, `max_altitude`, `max_speed`,
  `home_lat/lon`, `point_count` and `gps_track` **geometry**: never changed by
  the parser contract. The single exception in this whole plan is a P7 row that
  is deliberately replaced from a recovered original (§7).

---

## 3. Backfill and repair of existing rows

Three separate write sets with three separate guarantees. Keeping them separate
is the point — a bug in one cannot reach the others.

### 3.1 What exists today, and why it is the wrong tool

`FlightDataTab.tsx:522-560` surfaces the reprocess mechanism: a "Need Reprocess"
stat from `GET /flight-library/reprocess/status`
(`flight_library.py:1589-1627`), a "RE-PROCESS N FLIGHTS" button hitting
`POST /flight-library/reprocess/all` with a 600 s client timeout
(`FlightDataTab.tsx:215`), and a manual re-upload fallback. (The "Restart (Home)"
/ "Skip +5%" controls sometimes attributed to this tab are playback buttons in
`FlightReplay.tsx:534,562` — a different surface.)

**`/reprocess/all` is not reused**, for two independent reasons:

1. **Its selector matches none of them.** It targets
   `point_count == 0 OR point_count IS NULL OR gps_track IS NULL`
   (`flight_library.py:1652-1658`). All 210 DJI flights have decoded frames (but only 182 have a retained file, see §8) and
   tracks.
2. **Its write set is exactly what must not move.** It assigns `duration_secs`,
   `total_distance`, `max_altitude`, `max_speed`, `home_lat/lon`, `point_count`,
   `gps_track`, `telemetry`, `raw_metadata` (`flight_library.py:1699-1709`) —
   the values ADR-0027 and ADR-0028 settled and migration `0004` restamped.
   Re-deriving them from a different parser build is a silent-regression
   generator.

### 3.2 Write set A — details + series (`POST /details/backfill`)

`?limit=25&force=false&dry_run=false`, auth `Depends(get_current_user)` like
every neighbouring route.

```
1. select(Flight.id, Flight.source_file_hash)             # COLUMNS, not the entity
     .outerjoin(FlightDetails, FlightDetails.flight_id == Flight.id)
     .where(Flight.source == "dji_txt",
            Flight.source_file_hash.is_not(None),
            FlightDetails.flight_id.is_(None))            # force=true drops this
     .limit(limit)
2. _get_stored_file_path(hash)  -> skip if absent
3. stream the stored original to the parser from an open file handle
     (ADR-0028 M5 pattern, flight_library.py:1676-1683 — never read_bytes())
4. parsed["details"]; skip (not error) when absent
5. async with db.begin_nested():                          # ADR-0028 C2 savepoint
       upsert FlightDetails; delete+insert FlightSeries rows for this flight
6. return {processed, written, skipped_no_file, skipped_no_details, remaining, errors}
```

**Guarantee: structural.** The `Flight` ORM entity is never loaded, so there is
no object to dirty and no attribute assignment is possible. Not "we remembered
not to write `duration_secs`" — there is nothing there to write to. Step 1 is
also the ADR-0019/0025 lesson applied literally.

**Idempotent** via the `FlightDetails.flight_id IS NULL` clause; `force=true`
re-parses and upserts (needed after a parser or crate bump, and
`parser_version` / `crate_version` make "re-backfill everything below version X"
a trivial query). Series are replaced wholesale per flight, never merged.

**Memory:** peak per iteration is one streamed original (never fully resident)
plus one details payload plus its series. Sequential, one flight at a time — no
fan-out, because the parser is a single 256 MiB container and the DJI keychain
API is a shared external dependency. `limit=25` keeps a request to roughly 1–2
minutes at ~2–4 s/parse, so the UI loops on `remaining > 0` instead of holding
one 600 s request open the way `/reprocess/all` does.

**Measure before continuing:** after the first 25-flight batch, record
`pg_total_relation_size('flight_series')` and `pg_column_size(values)` for the
largest flight in `PROGRESS.md`, and compare against §1.5's estimate before
running the remaining 185.

### 3.3 Fleet-level invariant checksum

Run on BOS-HQ against `droneops-standby-db` **before and after every write set**,
output quoted into `PROGRESS.md`:

```sql
-- headline scalars
SELECT md5(string_agg(
         id::text||'|'||duration_secs||'|'||total_distance||'|'||
         max_altitude||'|'||max_speed||'|'||point_count, ',' ORDER BY id))
FROM flights WHERE source = 'dji_txt';

-- track geometry (D5a writes inside gps_track, so scalars alone are not enough)
SELECT md5(string_agg(t.h, ',' ORDER BY t.id)) FROM (
  SELECT f.id, md5(string_agg(
           (p->>'lat')||'|'||(p->>'lng')||'|'||(p->>'alt'), ',' ORDER BY ord)) AS h
  FROM flights f, LATERAL jsonb_array_elements(f.gps_track::jsonb)
                          WITH ORDINALITY AS e(p, ord)
  WHERE f.source = 'dji_txt' GROUP BY f.id) t;
```

Both hashes identical across write sets A and B, or the pass is reverted and
re-planned. Write set C (§7) is the one exception and is scoped explicitly there.

### 3.4 Write set B — the repair pass (D5), `POST /details/repair`

This one **does** write to `flights`, so the §3.2 structural guarantee does not
apply and a different, equally strong mechanism is required.
`?dry_run=true` by default.

**B(a) — re-stamp `gps_track` / `telemetry` timestamps on the 210.**

The key move: **timestamps are merged into the EXISTING stored track, never
replaced by the new parse's track.** Coordinates therefore cannot change, because
they never come from the new parse at all.

```
1. load the Flight row (unavoidable here)
2. snapshot invariants: duration_secs, total_distance, max_altitude, max_speed,
   home_lat, home_lon, point_count, len(gps_track),
   and md5 of [(lat, lng, alt) for p in gps_track]
3. re-parse the stored original
4. PRECONDITION: len(parsed_track) == len(existing_track)
      and len(parsed_timestamps) == len(existing_telemetry["altitude"])
   -> if not, SKIP the row and record it. A parser build that yields a different
      point count on the same file is exactly the signal that something moved;
      skipping is correct, aligning would be guessing.
5. async with db.begin_nested():
      for i, p in enumerate(existing_track):
          p["timestamp"] = parsed_track[i]["timestamp"]        # ONLY this key
      existing_telemetry["timestamps"] = parsed_timestamps     # ONLY this key
      flight_details.gps_timestamps_restamped_at = utcnow()
      flush
      POST-ASSERT: recompute the coordinate md5 and re-read the seven scalars;
                   any difference -> raise -> savepoint rolls back THIS row
```

`duration_secs`, `total_distance`, `max_altitude`, `max_speed`, `point_count`
are never assigned — they are not in the write set. Every other `telemetry` array
is left as the stored object. `raw_metadata` is untouched; the audit stamp lives
in `flight_details` instead.

**B(b) — repair `drone_model = "Unknown(NNN)"`.**

150 of 210 DJI flights carry the literal because the crate's `ProductType` enum
predates the Mavic 4 Pro and Matrice 4 series; the real name is in
`raw_metadata.aircraft_name`.

- Predicate: `drone_model ~ '^Unknown\(\d+\)$'` — anchored, so a real model name
  can never match — **and** `raw_metadata->>'aircraft_name'` non-empty.
- Prior value preserved in `flight_details.drone_model_previous`.
- Idempotent: a second run matches nothing.
- Attribution is by serial (ADR-0007), so nothing was ever mis-attributed; this
  is cosmetic plus queryability.
- **Note, not a problem:** `_generate_flight_name` derives auto names from
  `drone_model` (`flight_library.py:666`). Existing names are already persisted
  and are **not** regenerated, so no flight is renamed and the
  `uq_flights_autoname` index (ADR-0027) is not touched. Only hypothetical future
  name generation would differ.

**Dry-run output** (both sub-passes): counts of rows that would change, rows
failing the length precondition, and a per-row before/after for `drone_model`.
Bill reviews it before the real run.

---

## 4. API

### 4.1 `GET /api/flight-library/{flight_id}/details`

- **Auth:** `Depends(get_current_user)` — identical to `get_flight` /
  `get_telemetry` / `get_track` (`flight_library.py:1972,1987,2024`).
- **Never 404s on a missing details row.** Returns
  `{source, details: {...}|null, series_index: [...], unavailable_reason}`.
  Per **D3** the link exists on every flight, so "no extended data" is a normal
  response, not an error. `unavailable_reason` is one of
  `null` / `"source_unsupported"` (litchi/airdata/manual) /
  `"not_backfilled"` (dji_txt with no row yet) /
  `"odl_import_no_original"` (opendronelog_import, pending P7).
- 404 only when the **flight** does not exist.
- `series_index` lists available series (`source`, `name`, `unit`,
  `sample_count`) **without** their values, so the page can render section
  availability in one round trip.

### 4.2 `GET /api/flight-library/{flight_id}/details/series`

- `?names=altitude_msl_m,rc_downlink&source=frame&max_points=2000` (cap 10000).
- Fetches only the requested `flight_series` rows and **downsamples at read**
  (D2) — index-stride, and `t_offset_s` for the same `source` is always returned
  alongside at the **same indices**, so the caller never has to align anything.
- The `downsample` closure currently inline in `get_telemetry`
  (`flight_library.py:1999-2003`) is **extracted** to
  `backend/app/services/telemetry_downsample.py` and shared by both endpoints.
  This is ADR-0032's standing finding applied prophylactically — that ADR's own
  conclusion is that the absence of a shared layer is what lets a defect class
  recur, and a second copy-pasted downsampler would be the same mistake.

### 4.3 Other endpoints

- `POST /details/backfill` — §3.2. `POST /details/repair` — §3.4.
- `GET /details/status` — `{total, by_source, with_details, without_details,
  with_stored_file, parser_versions, crate_versions, restamped, model_repaired}`.
  All `COUNT(*)` / `GROUP BY`; no JSON loaded. Mirrors `/reprocess/status`'s
  shape so `FlightDataTab` gains a second card that reads the same way.
- `POST /flights/reimport-odl` — §7.
- **The Flights list is unchanged.** Per D3 the link renders for every row using
  `source`, which `FlightResponse` already carries (`schemas/flight.py:65`). No
  new column, no join, no `has_details` flag, no risk to the query ADR-0019
  exists to protect.

### 4.4 Report-audience guard (D1) — explicit and testable

Pilot position is now stored in full. It must not reach a client deliverable
unless deliberately added.

1. **The report engine reads nothing from these tables.** `reports.py`
   (`generate_report:481`, `_build_flight_summaries:408`,
   `_resolve_flight_metrics:242`) resolves live scalars from `Flight` /
   `MissionFlight` and gains **no** access to `flight_details` or
   `flight_series`.
2. **A named allowlist, currently empty.** A module constant
   `REPORT_READABLE_DETAIL_FIELDS: tuple[str, ...] = ()` in the report layer
   documents that nothing from the details surface is report-eligible yet, and
   makes any future addition a deliberate, reviewable, one-line diff rather than
   an accidental import.
3. **A test asserts it.** Report generation issues **zero** queries against
   `flight_details` or `flight_series` — captured with a SQLAlchemy
   `before_execute` listener, the same technique as the §6 P3 no-UPDATE test.
4. **Exports too.** `_export_csv` / `_export_gpx` / `_export_kml`
   (`flight_library.py:2625,2645,2668`) are client-shareable artifacts and gain
   no pilot columns. Covered by the same test.
5. ADR-0029 / ADR-0031 remain in force everywhere: the Details page presents
   recorded height limits as data and never compares them to any limit, never
   mentions 400 ft or Part 107, and emits no compliance commentary.

**Consequence to accept knowingly:** pilot coordinates are PII-adjacent, live in
a database backed up to R2 (ADR-0041), and are recoverable from those backups.
The guard above is the mitigation, and `pilot_track_stored` makes the affected
rows enumerable if a purge is ever wanted.

---

## 5. Frontend

### 5.1 Where it hangs off the Flights menu

The precedent exists: nav item `Flights → /flights` (`AppShell.tsx:41`), a
detail `Drawer` opened by clicking a row (`Flights.tsx:771`), a per-row kebab
`Menu` (`Flights.tsx:808-829`), and a per-flight sub-page `/flights/:id/replay`
(`App.tsx:125`, lazy at `App.tsx:50`, launched from the drawer at
`Flights.tsx:888-899`).

Follow it exactly. **No new top-level nav item** — AppShell already carries 12,
and this is a per-flight view.

1. Route `/flights/:id/details` → `FlightDetails.tsx`, lazy, beside the replay
   route.
2. A `FLIGHT DETAILS` button in the drawer above `FLIGHT REPLAY`, same styling
   (cyan, `light`, `fullWidth`, Bebas Neue, 2 px tracking). **Rendered
   unconditionally** — D3.
3. A `Flight details` item at the top of the per-row kebab menu, above `Edit`
   (`Flights.tsx:813`).

### 5.2 Sections and per-source availability (D3)

Mantine `Card`s in the established style (`#0e1117` on `#1a1f2e`, Bebas Neue
headings, Share Tech Mono for data — `Flights.tsx:905-1000` is the pattern).

| Section | Content |
|---|---|
| **Summary** | Frames/records decoded, frame rate, parser + crate version, MSL max/min + home MSL, max VPS, max distance from home, max climb/descent. AGL max stays in the drawer, not duplicated. |
| **Flight phases** | One horizontal stacked bar (plain CSS flex, no chart library) + a state → seconds → % table. Takeoff/landing/RTH counts as badges. |
| **Camera** | Photo count with the header `capture_num` cross-check shown when they disagree (the M4TD census case: 5 edges vs header 4), video seconds, SD card remaining at landing. |
| **Battery** | Start/end/min V, max current, energy Wh, discharge mAh, cell deviation, temps; pack cycle count + designed capacity **hidden when `pack_values_plausible = false`**. |
| **Link quality** | Downlink/uplink min/avg/max, frames at zero downlink, disconnect count, OFDM average. Colour by band, no chart. |
| **Events & warnings** | Table: `t_offset_s` as mm:ss, severity badge, message, count. `garbled` rows carry a subtle marker so a mangled string never reads as a clean fact. |
| **Config** | Height limit in force, go-home height, max allowed height, beginner mode, failsafe, obstacle avoidance / MVO. **No commentary, no comparison** (§4.4 item 5). |
| **Firmware & serials** | Component firmware table, 20-char aircraft SN, RC/camera/battery SNs. |
| **Pilot position / VLOS** | Sample count, max and average aircraft-to-pilot distance. Coordinates are **stored but not displayed** on this page in P5; a pilot-position map layer is deferred (§5.3) and would be a deliberate addition. |

**Per-source rendering (D3).** Every section renders one of: data, or a muted
"Not available for `<source>` logs" line, or "No extended data yet — re-process
this flight" with a link to Settings → Flight Data. Availability by source:

| Source | Sections |
|---|---|
| `dji_txt` (backfilled) | all |
| `dji_txt` (not yet backfilled) | none — whole-page "not backfilled" state |
| `litchi_csv`, `airdata_csv`, `manual` | none — "not available for this source" |
| `opendronelog_import` | none today; **all** once P7 replaces the row from a recovered original — which makes P7's value visible in the UI |

### 5.3 Explicitly deferred (do not build in P5)

Chart polish and any charting library; map overlays (link-quality-coloured track,
pilot position layer, gimbal footprint); mission-report integration; maintenance
triggers from `health` flags; event → ntfy alerting; stick-position analysis;
cross-flight comparison views.

---

## 6. Build order, sizing, tests, versions, deploy

Sizing is calendar-days of focused work.

| Phase | Scope | Size | App ver | Parser ver |
|---|---|---|---|---|
| **P0** | Two tables + read path + encoding measurement gate | M ~1 d | 2.82.0 | — |
| **P1** | Tier 0 parser pass, full-res series, rounding, `drone_model` fallback | M ~2 d | 2.83.0 | 1.2.0 |
| **P-EVAL** | Crate before/after diff → report in `docs/reports/` | M ~1 d | — | — |
| **P2** | Tier 1 records pass (spike-gated) | M–L ~2 d | 2.84.0 | 1.3.0 |
| **P3** | Backfill — details + series | M ~1 d | 2.85.0 | — |
| **P4** | Repair pass — timestamps + `drone_model`, dry-run first | M ~1 d | 2.86.0 | — |
| **P5** | Flight Details UI | M ~2 d | 2.87.0 | — |
| **P6** | Battery source of truth (un-gated per D4) | M ~1 d | 2.88.0 | — |
| **P7** | ODL re-import / replace, dry-run first | L ~2 d | 2.89.0 | — |

**Total ≈ 13 days.** P2 carries the technical risk; P7 carries the schedule risk
(blocked on §8).

**Ordering notes.** **P-EVAL runs before P2** deliberately: if a newer crate
fixes `SmartBatteryStatic` and `ProductType`, then P2's shim and part of P4(b)
shrink or vanish, and building the shim first risks writing code we delete a day
later. **P5 (UI) may be pulled ahead of P4** if Bill wants eyes on the page
sooner — it depends on P3 only. **P6 depends on P2** (pack values are Tier 1).
**P7 depends on P0–P3 and on the §8 inventory.**

**P0 — schema + read path.** Migrations `0010` (both tables) and `0011`
(battery columns, landed early so P6 needs no migration), models,
`Flight.details` / `Flight.series` with `lazy="noload"`, schemas, `/details`,
`/details/series`, `/details/status`, the extracted `telemetry_downsample`
service, and the **§1.5 encoding measurement**.
*Tests:* migrations no-op on a fresh DB; the compiled `/flight-library` list
query references neither new table (mirroring
`test_flight_library_list_defers_heavy_columns.py`, control test included);
`/details` shapes for each `unavailable_reason`. Inert on deploy.

**P1 — Tier 0.** All of §2.2 in the existing frame loop, §2.5 rounding,
`details: None` in `litchi.rs` / `airdata.rs`, the `Unknown(NNN)` →
`aircraft_name` fallback. The backend persists `parsed["details"]` in
`_build_flight_from_parsed` (`flight_library.py:422-534`) inside the existing
best-effort savepoint pattern used for battery tracking
(`flight_library.py:527-533`) — a details failure must never fail an import.
*Rust tests:* phase histogram over a synthetic frame vector; photo counter counts
**rising edges only**; event dedupe collapses N identical strings to one record
with `count = N`; garbled-prefix trim and flag; per-quantity rounding precision;
ADR-0032 unit assertions.
*pytest:* a payload with `details` writes one `flight_details` row and N
`flight_series` rows; one without writes none; malformed `details` is swallowed
and the flight still imports.

**P-EVAL — library evaluation (D6). EXECUTED 2026-09-11 —
[`docs/reports/2026-09-11-dji-log-parser-upgrade-eval.md`](../reports/2026-09-11-dji-log-parser-upgrade-eval.md).**
Outcome: **no crate bump exists.** The newest published `dji-log-parser` is
`0.5.7`, already the pinned version; upstream is one unreleased commit ahead
(Inspire-1 battery serials) and dormant since 2025-06-07. That commit was
evaluated anyway — 782 comparisons over 762 distinct logs, 24 metrics,
6,756,743 `gps_track` coordinates, **zero differences** — and not adopted.
Neither hoped-for fix landed upstream, so **step 8 below does not apply**,
`Unknown(NNN)` does not resolve, and **P2 must build the §2.4 shim in full**.
Three figures in the block below are stale: the corpus is **198** real originals
(not 182), `dji_txt` is **226** rows (not 210), and `flight-parser/Cargo.toml`
requests `"0.5"` rather than pinning `0.5.7` — the lockfile is the only thing
enforcing D6. The original specification follows unchanged.

A standalone harness, **no DB writes**.
1. Resolve the newest published `dji-log-parser` (`cargo search` / crates.io) —
   do not assume a version number.
2. Build the parser twice: pinned `0.5.7` and the candidate.
3. Run both over every retained original (the 182 real files in the backend container's
   `/data/uploads/flight_logs/`, plus anything recovered per §8).
4. Diff per flight: `duration_secs`, `total_distance`, `max_altitude`,
   `max_speed`, `home_lat/lon`, `point_count`, **every `gps_track` coordinate**,
   `product_type`, `frames_decoded`, `frame_count`.
5. **Classify** each difference as *expected improvement* (e.g.
   `Unknown(178)` → `Matrice4TD`) or *unexplained*.
6. **Adoption rule:** adopt only if every headline metric is bit-identical, or
   every difference is individually explained and operator-approved.
7. **Output:** `docs/reports/2026-09-XX-dji-log-parser-upgrade-eval.md` — a new
   `docs/reports/` directory, consistent with the existing `docs/incidents/`,
   `docs/runbooks/`, `docs/ops/` split (this is a report, not a plan).
8. If adopted → its own ADR (0044) + a full `force=true` re-backfill, and only
   then is the §2.4 shim version-gated.

*Scope split:* the `SmartBatteryStatic` comparison needs the records API, which
is P2's spike — so P-EVAL's adoption gate covers headline metrics and
`product_type` (frames only, both versions), and the pack-value comparison folds
into P2's spike, run against whichever crate P-EVAL selected.

**P2 — Tier 1.**
*P2-a, the gate (first, alone):* the record accessor's signature; does it consume
the keychains; peak RSS on the largest prod log; does the selected crate decode
`SmartBatteryStatic` correctly. **Measured, not assumed.** If peak RSS exceeds
the parser's `mem_limit: 256m` (`docker-compose.yml:350`, sized for an idle
~1 MiB service), bump to 512m in the same change **justified by the
measurement** — not pre-emptively on a guess.
*P2-b:* the record pass, the `>> 8` shim with its plausibility gate, the pilot
series (D1).
*Tests:* the shim against all four census values (5888→23, 2304→9, 1899520→7420,
1505282→5880) plus rejection of an implausible result; `AppGPS`-absent logs
(Goggles) yield NULL pilot fields, not zeros; pilot series row count matches
`pilot_sample_count`.

**P3 — backfill.** §3.2 endpoint plus a `FlightDataTab` card looping `limit=25`
until `remaining == 0`.
*Tests:* a `before_execute` listener asserts **zero** `UPDATE` against `flights`;
a second run reports `processed: 0`; a missing stored file is a skip, not an
error; `force=true` replaces series wholesale rather than appending.
*Prod:* §3.3 checksums before/after, plus the §3.2 size measurement after batch 1.

**P4 — repair pass.** §3.4, `dry_run=true` by default.
*Tests:* the length precondition skips rather than aligns; the post-assert rolls
back a row whose coordinate hash moved (forced with a stubbed parse); `drone_model`
predicate never matches a real model name; second run is a no-op.
*Prod:* both §3.3 checksums identical before/after — the geometry hash is the one
that matters here.

**P5 — UI.** Route, page, two entry points, per-source availability.
*Test:* one Vitest smoke at `frontend/src/pages/__tests__/FlightDetails.test.tsx`
(the established location) — renders each section from a fixture, and renders the
correct message for each `unavailable_reason`.

**P6 — battery source of truth (D4).** §1.6. No migration (landed in P0's `0011`).
*Tests:* the `GREATEST` monotonic guard survives out-of-order backfill;
`cycle_count_observed` is seeded exactly once; `health_pct` is NULL outside
40–105 %; `pack_values_plausible = false` never becomes a source.

**P7 — ODL re-import.** §7, `dry_run=true` by default. Blocked on §8.

**CI:** `.github/workflows/` holds only `auto-merge-claude.yml`,
`secret-scan.yml`, `self-hosted-smoke-test.yml` — **no pytest or cargo job.**
Each phase runs `cd flight-parser && cargo test` and `cd backend && pytest`
locally and **quotes the output** in the commit body or `PROGRESS.md`. An exit
code is not evidence.

**Version bumps** — CLAUDE.md requires all four: `README.md:5`,
`frontend/package.json:4`, `backend/app/main.py:602`, and
`frontend/src/components/Layout/AppShell.tsx`. Note the AppShell string appears
at **two** places (lines 121 and 394 — desktop sidebar and mobile drawer) while
CLAUDE.md says "line" singular; both must move. Additionally
`flight-parser/Cargo.toml` (currently `1.1.0`) bumps on P1/P2 — it is **not** in
CLAUDE.md's four-file list and should be added as a fifth marker, because
`main.rs:127` already reports `env!("CARGO_PKG_VERSION")` on `GET /health`, which
is how a parser deploy is confirmed.

**Deploy.** Prod is **not** deployed by hand. Per **ADR-0018** and CLAUDE.md,
DroneOpsCommand prod on BOS-HQ is deployed by the **NOC Master Control fleet
deployer** (`swarmpilot_deployer`) on push to `main`; the `.deployer-disabled`
marker in the repo root disables the retired *per-repo autopull*, not the fleet
deployer. `update.sh` does not exist (deleted in `e4610b5`) — **CLAUDE.md's
"Tech Stack → Deploy: `update.sh`" line is stale and wants a docs pass.**
`flight-parser` is a `build:`-only compose service
(`docker-compose.yml:344-347`); NOC ADR-0079 taught the deployer's digest gate to
resolve those, so a Rust change does rebuild. Verify each parser deploy with
`GET /health` → `version` (`main.rs:120-133`), not with "the push succeeded."
The **demo stack** (`~/droneops-demo`) is not deployer-managed and is updated by
hand per CLAUDE.md.

Docs-only commits carry `[skip-deploy]` in the subject.

---

## 7. P7 — OpenDroneLog re-import and replacement (D7)

584 `opendronelog_import` rows exist with `raw_metadata = NULL` — never through
the Rust parser (ADR-0028 baseline). Where an original DJI file is recovered
(§8), re-import it and replace the matched row.

### 7.1 Matching rule

Deliberately strict: a wrong match destroys a mission attachment.

For each candidate file, parse → `drone_serial`, `start_time`, `duration_secs`,
then find rows where **all** hold:

- `source == 'opendronelog_import'`
- `drone_serial` matches on a **16-char prefix**, after trim + case-fold —
  **NOT equality.** See the correction note below; equality matches zero rows.
  A row with **no** serial is never matched (the match would be too weak); it
  imports as new and is flagged.
- `abs(start_time - parsed_start) <= 120 s`
- `abs(duration_secs - parsed_duration) <= max(30 s, 5 % of parsed_duration)`

Then:

| Candidates | Action |
|---|---|
| exactly 1 | **match** → replace |
| 0 | **import as new flight** |
| ≥ 2 | **NO MATCH** — flag for manual review, never guess |

**Why the uniqueness rule is not optional.** ADR-0027 measured **44–184 s**
battery-swap gaps between consecutive Savannah flights on the same airframe — so
a ±120 s start window *can* reach an adjacent flight. The duration test does most
of the disambiguation, and the "≥ 2 → abort" rule guarantees an ambiguous pair is
rejected rather than resolved by nearest-neighbour. Tolerances are generous on
purpose: ODL `start_time` provenance is unknown and may be ingest-derived or
timezone-shifted (ADR-0017 is about exactly this class).

**CORRECTION (2026-09-05, verified against the BOS-HQ prod DB
`droneops-standby-db`, db `droneops` — note that is the promoted standby, not
`droneops-db-1`, which is an `alpine:3` placeholder).** The first draft of this
rule said `drone_serial` must be **equal**. It cannot be, and as written the
rule matched **zero of the 584 files**:

DJI serials exist in two forms in this database. Every `opendronelog_import`
row carries the 16-char header serial **plus a 4-char suffix**:

| Airframe | header (16) | stored ODL form (20) |
|---|---|---|
| Matrice 30T | `1581F5BK7241J00B` | `1581F5BK7241J00BA040` |
| Mavic 3 Pro | `1581F67QC23CN014` | `1581F67QC23CN014E4Z8` |
| Matrice 4TD | `1581F8HGX255P00A` | `1581F8HGX255P00A0FEK` |
| Mini 5 Pro | `1581F9DEC257N029` | `1581F9DEC257N0293HSX` |
| Matrice 4T | `1581F7K3C25AA00D` | `1581F7K3C25AA00DMZMG` |
| Avata 2 | `1581F6W8A242N0A3` | `1581F6W8A242N0A3YVZ8` |
| M3P (DECOM) | `1581F67QE236L00A` | `1581F67QE236L00A0027` |

The 14-char FPV serials carry no suffix and are identical in both forms. A
re-parsed file yields the **16-char header** form (`details.aircraft_sn` is 16
bytes for log version > 5), so the comparison against a stored 20-char value
must be a prefix test. §8a already moved the *primary* lookup to
`original_filename` (1:1, 584/584); this correction fixes the serial
**verification** step, which would otherwise have rejected every match §8a
found.

Two knock-on corrections to this document:

- The claim elsewhere that the two missing aircraft rows account for **46**
  unattributed flights is **wrong**. The verified breakdown of the 88
  unattributed ODL rows is 49 Matrice 4TD + 39 Matrice 4T. The 4TD's aircraft
  row has existed since 2026-03-16 with serial `1581F8HGX255P00A`; those 49 are
  unattributed *solely* because of this 16-vs-20-char mismatch, not a missing
  row.
- `_match_fleet_aircraft` (`flight_library.py`, branch 1) compares
  `func.upper(Aircraft.serial_number) == drone_serial.upper()` via
  `scalar_one_or_none()` — **exact equality, no prefix logic.** Nothing in
  P0–P1 changes it, and nothing built there assumes the two forms interoperate.
  `flight_details.aircraft_sn_full` (Tier 1, §2.3) is where the 20-char form
  will land and is the natural place to reconcile the two later — as a
  deliberate change with its own decision record, not a silent widening of the
  matcher.

**Hash collision check.** If the file's SHA-256 already exists on another flight
(the partial unique index `uq_flights_source_file_hash`, migration `0005`), do
**not** replace — report `duplicate_of=<flight_id>`. That means the physical
flight exists twice (an ODL row and a DJI row); merging them is a destructive
call for Bill, not for the importer.

### 7.2 Replacement mechanism — update in place

Two shapes were considered:

- **(A) Update in place** — keep the existing `flights.id`, overwrite the parsed
  fields, flip `source` to `dji_txt`, set `source_file_hash`.
- **(B) Insert new + migrate foreign keys + delete old.**

**(A), decisively.** Every carry-over Bill asked for is **free** under (A)
because the primary key never changes: `mission_flights.flight_id`,
`battery_logs.flight_id` and every other reference survive untouched, and
`notes`, `tags`, `pilot_id`, `aircraft_id` persist simply by not being assigned.
Under (B) each of those is a manual FK migration with a chance to miss one.

**One wrinkle (A) creates and must handle:** `mission_flights.flight_data_cache`
holds scalar display fields snapshotted at attach time
(`missions.py:_scalar_cache_from_flight:85`, ADR-0025 A2). The replaced flight's
duration/distance **will** change — that is the point — so the cache goes stale.
Reports resolve live scalars (ADR-0028 H2) but the editor reads the cache for
display, so **every affected `mission_flights` row's scalar cache is refreshed**
as part of the replace, via the same helper.

### 7.3 The one place "nothing moves" does not apply

A replaced row's headline metrics **do** change: ODL passthrough values (some
already sanitized by ADR-0028 C1 / migration `0006`) give way to a real DJI parse.
That is the intent. Scoping and evidence:

- It applies **only** to rows with a unique file match. Every other
  `opendronelog_import` row is untouched.
- The §3.3 checksum invariant covers `source = 'dji_txt'` and is therefore
  unaffected; a separate before/after snapshot is taken over the matched ODL row
  set specifically.
- Prior values are preserved in `flight_details.replaced_from` — prior `source`,
  the five headline scalars, `point_count`, and any
  `raw_metadata.distance_sanitized` note.
- ADR-0031 verified ODL `max_altitude` against per-point tracks for 570 flights,
  so ODL altitudes are trustworthy today and the DJI parse should broadly agree.
  **Large disagreements are a signal, not noise** — the dry-run reports per-match
  metric deltas so Bill can eyeball them before committing.

### 7.4 Dry run

`POST /flights/reimport-odl?dry_run=true` (default) returns the full match table
— file → candidate flight → decision (`match` / `new` / `ambiguous` /
`duplicate` / `no_serial`) → per-metric deltas → affected `mission_flights` count
— with **zero** writes. Nothing is committed until Bill has read it.

---

## 8. Log inventory — FILLED IN 2026-09-04 (fleet-wide hunt, read-only)

Hosts searched: HSH-HQ, BOS-HQ, svdp-dev/CHAD, Ocean, SDR Pi, Dev-Ops-2, the
Synology RS1221+ (docker volumes, Downloads, Active Backup share listing), and
the UNAS SMB share. Not reachable: **NEXTL3VEL** (Bill's Windows 11 PC,
192.168.50.74, offline during the hunt).

### Where raw DJI logs exist

| # | Host / path | Files | Date range | Already ingested? |
|---|---|---|---|---|
| 1 | BOS-HQ `droneops-backend-1:/data/uploads/flight_logs` (docker volume `droneops_app_data`) | **184** = 182 real logs + 2 dummy test files (40 B / 70 B, not in DB) | 2023-12-25 → 2026-09-01 | Yes, all 182 hashes are `dji_txt` rows |
| 2 | BOS-HQ restic repo (R2 `droneops-backups`, 48 snapshots since 2026-08-17) | union across all snapshots = the same 184 | same | Yes, nothing beyond #1 |
| 3 | Legacy plaintext mirror `s3://obs-glitchtip-backups/droneops/uploads/flight_logs/` | 183 | same | Yes, subset of #1 |
| 4 | Synology docker volume `droneops_app_data` (dead 2026-03 instance) and HSH `~/migration/doc_appdata.tar.gz` | 24 | 2023-12-25 → 2026-03-22 | Yes, all 24 already on BOS |
| 5 | Synology Downloads, UNAS `\Drone LOGS` (empty folder, created 2026-04-04), UNAS `\Drone Master Archive`, svdp-dev volumes, Ocean, SDR, Dev-Ops-2, DroneOpsSync / DroneOpsMap checkouts | **0** logs (only a KML, a GPX/JSON export and an ODL signature PNG) | | |

**Net: 182 distinct usable originals exist anywhere on the fleet, all on BOS-HQ,
all already ingested. No archive of logs exists on any host.**

### Two gaps, with evidence

**(a) 28 of the 210 `dji_txt` rows have no original file.** Verified by hash
set comparison (DB 210, files 184, DB-without-file 28, file-without-DB 2 = the
dummies). All 28 were created 2026-03-24 → 2026-04-19 on the HSH-HQ prod era.
The 2026-04-20 HSH→BOS migration promoted the BOS standby DB but brought up a
fresh, empty `droneops_app_data` volume on BOS; HSH's volume was never copied
and no longer exists; backups only began 2026-07-16. DroneOpsSync deletes the
controller copy after a confirmed sync. **These 28 cannot be backfilled from
file.** Their Flight Details will show "original log not retained" unless the
files resurface. Every row from 2026-04-24 onward has its file.

**(b) The 584 OpenDroneLog-era originals were never on the fleet.** ODL ran as a
container on the Synology (`system_settings.opendronelog_url` =
`http://192.168.50.20:3001/`, now gone). Its DuckDB survives in an orphaned
Synology docker volume (`flights.db`, 468 MB, 548 flights, 4.66 M telemetry
rows, last written 2026-02-28) but ODL stores only `file_name` + sha256, never
the bytes. All 548 were imported into ODL in one batch on 2026-02-22 19:13–20:08
PST from a folder of raw files on a client device. DroneOpsCommand then pulled
metadata + track over ODL's REST API on 2026-03-18 (584 rows; 548 + 36 added to
a later ODL instance whose volume is gone). Only 17 of the 548 ODL hashes are on
BOS today (re-uploaded later as `dji_txt`).

**CORRECTED 2026-09-05 — the recovery hunt finished; several facts above are
wrong, and the mechanism behind gap (a) is now proven rather than inferred.**

*Counts moved (re-verified against the live prod DB on 2026-09-05, not taken
from this section):* `dji_txt` is **218**, not 210 — 8 flights were uploaded
2026-09-04 23:47 PDT. Retained originals are **192**, not 184/182. The S3
mirror holds **184**, not 183. **The 28 file-less rows are unchanged.** Every
"182" in the bullets below and elsewhere in this document should be read as
**192 minus the 2 dummies = 190 real originals**; re-derive from the database
rather than from any figure written here.

*Gap (a)'s cause is firmer than "backups only began 2026-07-16".* Of the 52
`dji_txt` rows created 2026-03-23 → 2026-04-19, exactly 24 have files, and that
set of 24 is byte-identical to the contents of `~/migration/doc_appdata.tar.gz`.
**The survival boundary is not a date — it is "was the file inside the migration
tarball."** Decisive evidence: HSH's `~/backups/pg-backup.sh` still exists and
runs only `pg_dump` plus an n8n sqlite `.backup`; it never touched
`/data/uploads` or the app-data volume. The HSH prod era therefore had **no
file-level backup of flight logs at all** — the bytes were never captured. The
28 are unrecoverable from any fleet source.

*The `original_filename` values for all 28 are recovered and follow **three
distinct naming patterns**,* so a hunt for `DJIFlightRecord*` alone misses 12 of
them. Manifest and full report live in the recovery session's scratchpad
(`missing_28_full.tsv`, `FP1-log-recovery-hunt-2026-09-05.md`); the manifest
should be copied into `docs/plans/data/` before that scratchpad is reaped.

*Two claims in this section are simply wrong and are corrected here:*

- **DroneOpsSync is an Android APK that runs on the controller, not a Windows
  companion app** — a Windows companion was formally *rejected* in DroneOpsSync
  ADR-0007. **NEXTL3VEL was never in the ingest path**, which weakens it
  considerably as a lead. (The likely seed of the confusion is a comment at
  `backend/tests/test_flight_ingest_consolidated.py:11` describing it as "the
  Windows companion"; corrected on this branch.)
- **The Synology Active Backup version range is wrong.** Not
  2026-05-29 → 2026-09-01: there are exactly **10 versions, 2026-05-29 →
  2026-06-07** (verified on the NAS), retention is 10 versions, and the task has
  logged missed backups **daily since 2026-06-08**. Shell enumerability of that
  repo is **nil** — each version holds only `0.img.delta`, `backup_db.sqlite`
  and `device_spec`, and the sqlite carries no file catalog, so the Active
  Backup portal is the only path. `device_spec` shows the image covers disk0 /
  one NTFS `C:\` only.

**Only remaining lead for both gaps:** the Windows PC **NEXTL3VEL** (the
2026-02-22 import folder, and any FlightRecord copies from the phones/RCs), plus
its Synology Active Backup for Business image repo
(`/volume1/ActiveBackupforBusiness/ActiveBackupData/PC-NEXTL3VEL-bbarnard065-Default`,
nightly image-level, versions 2026-05-29 → 2026-09-01; file-level restore is
possible from the Active Backup portal, not from the shell). Unverified until
the PC is on or the portal is searched. Also worth checking: the FlightRecord
folders on each phone/RC (`Android/data/dji.go.v5/files/FlightRecord`,
`Android/data/com.dji.fly/files/FlightRecord`, DJI Pilot 2's
`DJI/com.dji.industry.pilot/FlightRecord`) for anything DroneOpsSync never saw.

### Consequences for this plan

- Every "210 retained originals" figure elsewhere in this plan and ADR-0043 is
  stale. **Re-derive the count from the live database at run time** rather than
  hard-coding one: it was 182 on 2026-09-04 and 190 on 2026-09-05, and it moves
  every time Bill uploads. Backfill (A and B) and P-EVAL run over whatever has
  a stored file; the file-less `dji_txt` rows are skipped and reported, never
  synthesised.
- P7 now has **584 candidate files** on BOS (see §8a); 564 new, 20 duplicates.
- Recovery of either gap is an operator action (turn on NEXTL3VEL, search the
  Active Backup portal for `DJIFlightRecord_*` / `FlightRecord_*`). If found,
  copy the folder to a fleet host and hash-compare against the 184 before P7.

### 8a. RECOVERED 2026-09-04 (afternoon): the 584 OpenDroneLog originals

Bill had the folder. He shared it as a Google Drive folder ("Drone LOGS",
584 files, 2.34 GB, 2023-08-01 → 2026-03-17). Pulled to **BOS-HQ
`~/droneops-staging/drive-logs/`** (outside the app volume, original filenames
kept; manifest, sha256 list and header CSV alongside in `~/droneops-staging/`).
Full inventory with hashes and header metrics:
`docs/plans/data/2026-09-04-drive-logs-inventory.csv`.

Verified facts:
- **584 files, 584 distinct hashes, all log v14, all parse** with the current
  crate and the production key (7-sample deep census + header pass on all 584).
- **Filenames match the 584 `opendronelog_import` rows' `original_filename`
  1:1** (584/584, and `original_filename` is unique across those rows). This
  makes P7's match deterministic: **match on `original_filename` first**; the
  serial + start + duration rule becomes the verification, not the lookup.
- **548 of 584 hashes equal the sha256 OpenDroneLog recorded** in its DuckDB,
  i.e. byte-identical to what ODL ingested. The remaining 36 are the flights
  added to the later ODL instance whose DB is gone; they match by filename.
- **20 of 584 are already on BOS** as `dji_txt` rows (hash match) → P7 must
  treat those as duplicates of existing native flights, not replacements.
  **564 files are new to the fleet.** None of the 28 file-less `dji_txt` rows
  are among them (different era).
- Airframes (by header serial, 16-char form; ODL rows carry the 20-char form):
  Matrice 30T "Maverick" 206, Mavic 3 Pro "Badass V.2" 134, Mavic 3 Pro
  "Bad Mother Fucker" 78 (aircraft row "M3P - DECOM"), Matrice 4TD 50,
  Mini 5 Pro "BigThingsSmallPackages" 46 (`ProductType` 139), Matrice 4T 39
  (`ProductType` 150), Avata 2 21, DJI FPV 10 across two serials.

  **CORRECTED 2026-09-05 (verified on prod).** Both missing aircraft rows were
  created on 2026-09-05 with the 16-char header serial — `DJI Matrice 4T` /
  `1581F7K3C25AA00D` and `DJI FPV` / `37Q7LA800BX0PN` — and the pre-existing
  `37QBJ5WBD100DN` row was renamed `DJI FPV - DECOM` so branch 2 of
  `_match_fleet_aircraft` (the no-serial model fallback) cannot go ambiguous on
  two identically-named rows. 9 flights on `37Q7LA800BX0PN` were re-pointed
  from the DECOM airframe to the new active one (they had been fuzzy-matched
  pre-ADR-0007; blast radius verified nil first). Aircraft table is now 11
  rows, zero duplicate serials.

  **The earlier claim that creating those two rows would attribute 46 flights
  was wrong.** The 88 unattributed ODL rows are 49 Matrice 4TD + 39 Matrice
  4T, and the Matrice 4TD has had an aircraft row since 2026-03-16. Both
  groups are unattributed for one reason only: the ODL rows store the 20-char
  serial while the aircraft rows store the 16-char header form, and
  `_match_fleet_aircraft` compares for **exact equality**. Creating rows does
  not fix that and was never going to. The new rows are the correct identity
  for P7's **re-imported** flights, which arrive carrying header serials — so
  they pay off at re-import, not today. Reconciling the existing 88 is a
  separate, deliberate decision (see the §7.1 correction).
- Totals: 135.0 h airtime, 2,215 photos by header count, 238 flights of 15–30
  min, 65 under 30 s (aborted takeoffs / tests). Header `max_height` exceeds
  120 m AGL on 349 of 584; spot-checked against frame data on the 2023 Mavic
  3 Pro log (500.1 m AGL, height limit 500 m) — the header values are real,
  not a units artefact. Recorded as data only (ADR-0029: reports are not
  compliance audits).
- The oldest log (DJI Fly 1.11.0, 2023-08) carries 8 embedded JPEG "moment
  pics"; newer app versions write none. Add JPEG extraction to Tier 1 as a
  low-priority optional item.
- `ProductType` 150 (Matrice 4T) is a third unknown code alongside 137/139/178.

Open before P7 runs — **both items updated 2026-09-05:**

1. ~~Create aircraft rows for the Matrice 4T and the second FPV.~~ **DONE** on
   2026-09-05, with the 16-char header serial; see the correction note above.
   Note this does **not** attribute the 88 existing unattributed ODL rows — the
   20-char serial mismatch is a separate decision (§7.1 correction).
2. ~~The staging copy is in no backup lane.~~ **CLOSED** 2026-09-05: the 584
   recovered originals in BOS `~/droneops-staging/` are now in the existing
   droneops restic repo under tag `staging` — snapshot `4c08afa7`, 2.181 GiB,
   in R2, independently verified. The source was mounted read-only; nothing was
   moved, deleted or reconfigured, so there is nothing for the deployer to
   clobber.

   **Caveat to carry:** that is a **one-shot snapshot of a static archive, not a
   recurring lane.** Anything added to `~/droneops-staging/` after 2026-09-05 is
   unprotected. The durable fix is still P7 ingesting the files into
   `/data/uploads/flight_logs/`, which the recurring lane already covers.

**New P7 lead (a lead, not a dependency).** `2026-03-06_20-38-33_Open_Dronelog.db.backup`
(95.6 MB, md5 `56a156aa…`) exists in two Synology Downloads archives alongside
`OpenDroneLog_Signature_576flights.png`. It is a week newer than the DuckDB
§8a cites and reports **576** flights, so it may carry recorded sha256s for
some of the 36 files that currently match by filename only — which would
upgrade those from a filename match to a **hash** match in §7.1's verification
step. Worth opening before P7's dry run; not required for P7 to proceed.

---

## 9. Risks and remaining unknowns

**Risks**

| # | Risk | Assessment | Mitigation |
|---|---|---|---|
| **R-1** | The `dji-log-parser` record-access API is unverified — signature, keychain consumption, return shape. | High likelihood of friction, impact confined to P2/P6. | P2-a is a hard gate. P0/P1/P3/P4/P5 deliver without it. If records need a second keychain fetch per flight, that is a per-flight external call across every backfill and P2 stops for a re-plan. |
| **R-2** | Parser memory — `mem_limit: 256m`, sized for an idle ~1 MiB service; frames + records + full-res series buffers on a 20 MB log could exceed it. | Medium/medium. An OOM-killed parser fails loudly (`httpx.ConnectError` → "flight-parser service unavailable"); it does not corrupt data. | Measure in P2-a; bump to 512m with the number in the commit body. |
| **R-3** | Storage — §1.5 is an estimate, not a measurement, and full resolution (D2) is ~10× the original decimated design. | Low impact; ~3× headroom before it matters. | P0 encoding measurement + the mandatory `pg_total_relation_size` check after backfill batch 1, before the remaining 185. |
| **R-4** | The `>> 8` shim rests on four data points from two airframes. | Medium likelihood of an airframe where it is wrong. | Plausibility gate + `pack_values_plausible`; the UI hides implausible values and D4 never consumes them. P-EVAL checks whether the shim is needed at all. |
| **R-5** | `Flight.details` / `Flight.series` changed from `lazy="noload"` to `selectin` would reproduce ADR-0019 across the whole list path. | Low likelihood, **high impact** (that was a production OOM crash-loop). | P0 test asserts the compiled list query references neither table, with a control test proving the assertion is meaningful. |
| **R-6** | **The repair pass (P4) writes to `flights`** — the first draft forbade this outright, and D5 authorizes it. | Low likelihood, **high impact**. | The §3.4 mechanism: merge-into-existing (coordinates never come from the new parse), a length precondition that skips rather than aligns, a post-write coordinate-hash assertion inside the savepoint, dry-run first, and both §3.3 checksums before/after. |
| **R-7** | **P7 mis-matches a file to the wrong ODL flight** and overwrites a real flight's data plus its mission attachment. | Low likelihood, **highest impact in the plan**. | Serial + start + duration, all three; `≥ 2 candidates → abort`; serial-less rows never matched; hash-collision check; `replaced_from` preserves the prior values; dry-run reviewed by Bill before any write. |
| **R-8** | Pilot coordinates are now stored and reach R2 backups. | Certain; impact is a privacy exposure, not an outage. | §4.4 report/export guard with a test; `pilot_track_stored` makes affected rows enumerable for a purge. |
| **R-9** | Tier 2 `Unknown` record types are ~30 % of records in newer logs. | Certain, low impact. | Out of scope, unchanged from the census. Type 48 (80 bytes, once per OSD frame on M4TD/M30) is the one to watch upstream. |

**Remaining unknowns — assumptions stated, not questions**

- **C-1 — `take_off_altitude` units.** The census found it appears to be **10×
  metres** on the M4TD; one airframe, one sample. **Assumption:** store raw,
  mark `take_off_altitude_units = 'unconfirmed'`, do **not** display. Guessing
  ×0.1 and being wrong puts a fabricated altitude on a screen, which ADR-0028's
  posture forbids. Closes when a second airframe confirms the scale — a
  by-product of P3, at no extra cost.
- **C-2 — `flight_series.values` encoding.** `json` vs `float8[]` is decided by
  the P0 measurement (§1.5), not by argument. Default `json`.
- **C-3 — event → alerting.** **Assumption:** no. Every event here is post-hoc
  and fails question 1 of the ADR-0037 five-question gate (actionable within 5
  minutes). If a "compass error on last flight" nudge is wanted later it belongs
  in the NOC digest, not as a page.

---

## 10. What this plan deliberately does not do

- Does not change `duration_secs`, `total_distance`, `max_altitude`,
  `max_speed`, `point_count` or `gps_track` **coordinates** on any existing row.
  The sole exception is a P7 row deliberately replaced from a recovered
  original, scoped and evidenced in §7.3.
- Does not change `flights.raw_metadata` (repair provenance lives in
  `flight_details`).
- Does not add a column to `flights`.
- Does not put a chart, a map overlay, or a report section anywhere.
- Does not display pilot coordinates, or expose them to reports or exports.
- Does not reinterpret, editorialize, or compare any altitude against any limit —
  ADR-0029 / ADR-0031 remain in full force on every surface this plan touches.
- Does not adopt a new `dji-log-parser` without P-EVAL's diff and its own ADR.
- Does not redefine `battery_logs.cycles_at_time` (the pack's value gets a new
  column instead).
