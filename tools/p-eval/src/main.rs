//! P-EVAL — `dji-log-parser` before/after evaluation harness (ADR-0043, D6).
//!
//! Runs TWO linked copies of the crate over the same log bytes with the same
//! keychain and diffs every quantity the plan names. It writes no database
//! rows, mutates no log file, and is not part of any deployed image.
//!
//! Why one binary rather than two builds: a single process reads the file
//! once, fetches (or loads) ONE keychain, and hands byte-identical input to
//! both crate versions. Two separate builds would need two keychain
//! round-trips per log, which both doubles load on DJI's API and introduces a
//! variable the experiment is trying to hold constant.
//!
//! The metric computation is a transcription of `flight-parser/src/dji.rs`.
//! The outlier gate is not transcribed — `gate.rs` is included verbatim from
//! the production tree below, so the one piece of real branching logic cannot
//! drift from what production runs.

#[path = "../../../flight-parser/src/gate.rs"]
mod gate;

use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Every quantity §6 P-EVAL step 4 names, plus the fields the candidate commit
/// actually touches (`battery_sn`) and the provenance needed to explain a diff.
#[derive(Serialize, Clone, PartialEq)]
struct Metrics {
    // ── the headline set ────────────────────────────────────────────────
    duration_secs: f64,
    total_distance: f64,
    max_altitude: f64,
    max_speed: f64,
    home_lat: Option<f64>,
    home_lon: Option<f64>,
    point_count: usize,
    product_type: String,
    frames_decoded: bool,
    frame_count: usize,
    /// sha256 over EVERY gps_track coordinate at full f64 precision, in order.
    /// A digest rather than 34k printed pairs: it covers every coordinate
    /// exactly, and `first_track_divergence` localises any mismatch.
    gps_track_digest: String,
    // ── fields the candidate commit can move, and provenance ────────────
    battery_sn: String,
    drone_model: Option<String>,
    aircraft_name: String,
    aircraft_sn: String,
    rc_sn: String,
    camera_sn: String,
    log_version: u8,
    start_time: String,
    header_duration_raw: f64,
    header_distance: f64,
    header_max_height: f64,
    header_max_hspeed: f64,
    dropped_segments: u64,
    keychain_source: String,
}

/// One lat/lng/alt/speed/heading tuple, captured so a digest mismatch can be
/// localised to an index instead of reported as an opaque "tracks differ".
#[derive(Clone, PartialEq)]
struct Pt {
    lat: f64,
    lng: f64,
    alt: f64,
    speed: f64,
    heading: f64,
    timestamp: Option<String>,
}

struct Reading {
    m: Metrics,
    track: Vec<Pt>,
}

/// Haversine distance in metres — verbatim from `dji.rs`.
fn haversine(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let r = 6371000.0;
    let dlat = (lat2 - lat1).to_radians();
    let dlon = (lon2 - lon1).to_radians();
    let a = (dlat / 2.0).sin().powi(2)
        + lat1.to_radians().cos() * lat2.to_radians().cos() * (dlon / 2.0).sin().powi(2);
    r * (2.0 * a.sqrt().atan2((1.0 - a).sqrt()))
}

/// `choose_duration` — verbatim from `dji.rs` (ADR-0027).
fn choose_duration(header: f64, fly: Option<f64>, dt: Option<f64>) -> f64 {
    if header > 0.0 {
        return header;
    }
    if let Some(s) = fly {
        if s > 0.0 {
            return s;
        }
    }
    if let Some(s) = dt {
        if s > 0.0 {
            return s;
        }
    }
    header
}

/// `is_unknown_product_type` — verbatim from `dji.rs`.
fn is_unknown_product_type(rendered: &str) -> bool {
    let Some(inner) = rendered
        .strip_prefix("Unknown(")
        .and_then(|r| r.strip_suffix(')'))
    else {
        return false;
    };
    !inner.is_empty() && inner.chars().all(|c| c.is_ascii_digit())
}

/// Generate `read_<name>` for one linked crate version.
///
/// A macro rather than a trait because the two crates are distinct types with
/// no shared supertrait — and because generating both arms from ONE body is
/// what guarantees the before and after readings are computed by identical
/// code. Anything that differs in the output therefore came from the crate,
/// not from the harness.
macro_rules! reader {
    ($fnname:ident, $crate_:ident) => {
        fn $fnname(
            bytes: &[u8],
            keychain_json: Option<&str>,
            keychain_source: &str,
        ) -> Result<Reading, String> {
            use $crate_::keychain::KeychainFeaturePoint;
            use $crate_::DJILog;

            let log = DJILog::from_bytes(bytes.to_vec())
                .map_err(|e| format!("from_bytes: {e}"))?;
            let d = &log.details;

            let aircraft_name_raw = d.aircraft_name.trim();
            let product_type = format!("{:?}", d.product_type);
            let drone_model = if product_type.is_empty() {
                None
            } else if is_unknown_product_type(&product_type) && !aircraft_name_raw.is_empty() {
                Some(aircraft_name_raw.to_string())
            } else {
                Some(product_type.clone())
            };

            let keychains: Option<Vec<Vec<KeychainFeaturePoint>>> = match keychain_json {
                Some(j) => Some(
                    serde_json::from_str(j).map_err(|e| format!("keychain decode: {e}"))?,
                ),
                None => None,
            };

            let frames = log.frames(keychains).unwrap_or_default();

            let mut track: Vec<Pt> = Vec::new();
            let (mut max_alt, mut max_speed, mut total_distance) = (0.0f64, 0.0f64, 0.0f64);
            let (mut prev_lat, mut prev_lon, mut prev_fly): (
                Option<f64>,
                Option<f64>,
                Option<f64>,
            ) = (None, None, None);
            let mut dropped_segments = 0u64;
            let (mut home_lat, mut home_lon): (Option<f64>, Option<f64>) = (None, None);
            let mut any_frame = false;

            for frame in &frames {
                any_frame = true;
                let osd = &frame.osd;
                let (lat, lon) = (osd.latitude, osd.longitude);
                let alt = osd.height as f64;
                let spd = ((osd.x_speed as f64).powi(2) + (osd.y_speed as f64).powi(2)).sqrt();

                if lat.abs() > 0.001 && lon.abs() > 0.001 {
                    track.push(Pt {
                        lat,
                        lng: lon,
                        alt,
                        speed: spd,
                        heading: osd.yaw as f64,
                        timestamp: Some(frame.custom.date_time.to_rfc3339()),
                    });
                    if home_lat.is_none() {
                        home_lat = Some(lat);
                        home_lon = Some(lon);
                    }
                    let cur_fly = osd.fly_time as f64;
                    if let (Some(pla), Some(plo)) = (prev_lat, prev_lon) {
                        let dist = haversine(pla, plo, lat, lon);
                        let dt = prev_fly.map(|p| cur_fly - p);
                        if gate::segment_ok(dist, dt) {
                            total_distance += dist;
                        } else {
                            dropped_segments += 1;
                        }
                    }
                    prev_lat = Some(lat);
                    prev_lon = Some(lon);
                    prev_fly = Some(cur_fly);
                }
                if alt > max_alt {
                    max_alt = alt;
                }
                if spd > max_speed {
                    max_speed = spd;
                }
            }

            let (fly_span, dt_span) = if frames.len() >= 2 {
                let (f, l) = (&frames[0], &frames[frames.len() - 1]);
                (
                    Some((l.osd.fly_time - f.osd.fly_time) as f64),
                    Some(
                        (l.custom.date_time - f.custom.date_time).num_milliseconds() as f64
                            / 1000.0,
                    ),
                )
            } else {
                (None, None)
            };

            let header_duration = d.total_time;
            let bounded = gate::sanity_bound_header(header_duration, dt_span);
            let header_distance = d.total_distance as f64;
            let header_max_height = d.max_height as f64;
            let header_max_hspeed = d.max_horizontal_speed as f64;

            // Full-precision ordered digest over every coordinate.
            let mut h = Sha256::new();
            for p in &track {
                h.update(
                    format!(
                        "{:?}|{:?}|{:?}|{:?}|{:?}\n",
                        p.lat, p.lng, p.alt, p.speed, p.heading
                    )
                    .as_bytes(),
                );
            }

            Ok(Reading {
                m: Metrics {
                    duration_secs: choose_duration(bounded, fly_span, dt_span),
                    total_distance: if any_frame && total_distance > 0.0 {
                        total_distance
                    } else {
                        header_distance
                    },
                    max_altitude: if any_frame && max_alt > 0.0 {
                        max_alt
                    } else {
                        header_max_height
                    },
                    max_speed: if any_frame && max_speed > 0.0 {
                        max_speed
                    } else {
                        header_max_hspeed
                    },
                    home_lat: home_lat.or(if d.latitude.abs() > 0.001 {
                        Some(d.latitude)
                    } else {
                        None
                    }),
                    home_lon: home_lon.or(if d.longitude.abs() > 0.001 {
                        Some(d.longitude)
                    } else {
                        None
                    }),
                    point_count: track.len(),
                    product_type,
                    frames_decoded: any_frame,
                    frame_count: frames.len(),
                    gps_track_digest: hex::encode(h.finalize()),
                    battery_sn: d.battery_sn.clone(),
                    drone_model,
                    aircraft_name: d.aircraft_name.clone(),
                    aircraft_sn: d.aircraft_sn.clone(),
                    rc_sn: d.rc_sn.clone(),
                    camera_sn: d.camera_sn.clone(),
                    log_version: log.version,
                    start_time: d.start_time.to_rfc3339(),
                    header_duration_raw: header_duration,
                    header_distance,
                    header_max_height,
                    header_max_hspeed,
                    dropped_segments,
                    keychain_source: keychain_source.to_string(),
                },
                track,
            })
        }
    };
}

reader!(read_pinned, djilog_pinned);
reader!(read_cand, djilog_cand);

/// Obtain the keychain for one log, preferring the on-disk cache.
///
/// Cached so the DJI keychain API is called at most ONCE per log for the whole
/// evaluation, and so a rerun costs nothing. The cache is also what lets both
/// crate versions be handed a byte-identical keychain: it is fetched with the
/// PINNED crate only, and the candidate deserialises the same JSON.
fn keychain_for(
    bytes: &[u8],
    hash: &str,
    cache_dir: &Path,
    api_key: Option<&str>,
) -> (Option<String>, String) {
    let cached = cache_dir.join(format!("{hash}.json"));
    if let Ok(j) = fs::read_to_string(&cached) {
        if !j.trim().is_empty() {
            return (Some(j), "cache".to_string());
        }
    }
    let Some(key) = api_key else {
        return (None, "no-key".to_string());
    };
    let log = match djilog_pinned::DJILog::from_bytes(bytes.to_vec()) {
        Ok(l) => l,
        Err(_) => return (None, "header-unparsable".to_string()),
    };
    match log.fetch_keychains(key) {
        Ok(kc) => match serde_json::to_string(&kc) {
            Ok(j) => {
                let _ = fs::write(&cached, &j);
                (Some(j), "fetched".to_string())
            }
            Err(_) => (None, "serialise-failed".to_string()),
        },
        Err(e) => {
            eprintln!("  keychain fetch failed for {}: {e}", &hash[..12]);
            (None, "fetch-failed".to_string())
        }
    }
}

#[derive(Serialize)]
struct FileResult {
    hash: String,
    file: String,
    corpus: String,
    size_bytes: u64,
    pinned: Option<Metrics>,
    cand: Option<Metrics>,
    pinned_error: Option<String>,
    cand_error: Option<String>,
    /// Metric names whose values differ between the two readings.
    differing_fields: Vec<String>,
    /// First gps_track index whose tuple differs, when the digests disagree.
    first_track_divergence: Option<usize>,
}

/// Field-by-field comparison. Floats are compared BITWISE (`to_bits`), not with
/// a tolerance: the adoption rule in §6 step 6 is "bit-identical", and a
/// tolerance would quietly absorb exactly the drift this harness exists to find.
fn diff_fields(a: &Metrics, b: &Metrics) -> Vec<String> {
    let mut out = Vec::new();
    macro_rules! f {
        ($n:literal, $f:ident) => {
            if a.$f.to_bits() != b.$f.to_bits() {
                out.push($n.to_string());
            }
        };
    }
    macro_rules! of {
        ($n:literal, $f:ident) => {
            match (a.$f, b.$f) {
                (Some(x), Some(y)) if x.to_bits() == y.to_bits() => {}
                (None, None) => {}
                _ => out.push($n.to_string()),
            }
        };
    }
    macro_rules! e {
        ($n:literal, $f:ident) => {
            if a.$f != b.$f {
                out.push($n.to_string());
            }
        };
    }
    f!("duration_secs", duration_secs);
    f!("total_distance", total_distance);
    f!("max_altitude", max_altitude);
    f!("max_speed", max_speed);
    of!("home_lat", home_lat);
    of!("home_lon", home_lon);
    e!("point_count", point_count);
    e!("product_type", product_type);
    e!("frames_decoded", frames_decoded);
    e!("frame_count", frame_count);
    e!("gps_track_digest", gps_track_digest);
    e!("battery_sn", battery_sn);
    e!("drone_model", drone_model);
    e!("aircraft_name", aircraft_name);
    e!("aircraft_sn", aircraft_sn);
    e!("rc_sn", rc_sn);
    e!("camera_sn", camera_sn);
    e!("log_version", log_version);
    e!("start_time", start_time);
    f!("header_duration_raw", header_duration_raw);
    f!("header_distance", header_distance);
    f!("header_max_height", header_max_height);
    f!("header_max_hspeed", header_max_hspeed);
    e!("dropped_segments", dropped_segments);
    out
}

/// Falsification controls. A run that reports "no field differed on 782 logs"
/// is worthless unless the harness can be shown to report a difference when one
/// exists — so this mode drives the ONE behaviour the candidate actually
/// changes, and the comparator, to a known-different answer.
///
/// Control 1 proves the two linked crates are genuinely different code.
/// Control 2 proves that difference is confined to the Inspire-1 product types.
/// Control 3 proves `diff_fields` reports differences rather than always [].
fn selftest() -> i32 {
    use djilog_cand::layout::details::{parse_battery_sn, ProductType};
    let mut failures = 0;

    // An Inspire 1 pack serial as the airframe writes it: low nibble of each
    // byte is a BCD digit, sequence reversed, leading zeros trimmed. The same
    // bytes read as UTF-8 (what 0.5.7 does, and what the candidate still does
    // for every other airframe) give something else entirely.
    let raw: Vec<u8> = vec![0x30, 0x39, 0x38, 0x37, 0x36, 0x35, 0x34, 0x33, 0x32, 0x31];
    let plain_utf8 = String::from_utf8_lossy(&raw)
        .trim_end_matches('\0')
        .to_string();
    let cand_inspire = parse_battery_sn(ProductType::Inspire1, raw.clone());

    println!("── control 1: candidate differs from 0.5.7 behaviour on Inspire1 ──");
    println!("  bytes              : {raw:02x?}");
    println!("  0.5.7 (utf8 decode): {plain_utf8:?}");
    println!("  candidate (BCD)    : {cand_inspire:?}");
    if cand_inspire == plain_utf8 {
        println!("  FAIL — candidate did not change the value; the two crates may be the same code");
        failures += 1;
    } else {
        println!("  PASS — a real behavioural difference exists and is observable");
    }

    println!("── control 2: the change is scoped to Inspire-1 product types ──");
    for pt in [
        ProductType::Avata2,
        ProductType::Matrice30,
        ProductType::Mavic3Pro,
        ProductType::Unknown(178),
        ProductType::Unknown(137),
        ProductType::Unknown(139),
    ] {
        let got = parse_battery_sn(pt, raw.clone());
        let ok = got == plain_utf8;
        println!("  {:<14} -> {:?} {}", format!("{pt:?}"), got, if ok { "same as 0.5.7" } else { "DIFFERS" });
        if !ok {
            failures += 1;
        }
    }
    for pt in [
        ProductType::Inspire1,
        ProductType::Inspire1Pro,
        ProductType::Inspire1RAW,
    ] {
        let got = parse_battery_sn(pt, raw.clone());
        let ok = got != plain_utf8;
        println!("  {:<14} -> {:?} {}", format!("{pt:?}"), got, if ok { "DIFFERS (expected)" } else { "FAIL same" });
        if !ok {
            failures += 1;
        }
    }

    println!("── control 3: diff_fields reports differences, not always [] ──");
    let base = probe_metrics();
    let same = diff_fields(&base, &base);
    println!("  identical inputs           -> {same:?}");
    if !same.is_empty() {
        println!("  FAIL — reported a difference between a value and itself");
        failures += 1;
    }
    let mut moved = probe_metrics();
    moved.total_distance += 0.000_000_1;
    moved.point_count += 1;
    moved.battery_sn = "DIFFERENT".into();
    moved.product_type = "Inspire1".into();
    moved.gps_track_digest = "deadbeef".into();
    moved.home_lat = None;
    let d = diff_fields(&base, &moved);
    println!("  six perturbed fields       -> {d:?}");
    let expected = [
        "total_distance",
        "point_count",
        "product_type",
        "gps_track_digest",
        "battery_sn",
        "home_lat",
    ];
    for e in expected {
        if !d.iter().any(|x| x == e) {
            println!("  FAIL — {e} was perturbed but not reported");
            failures += 1;
        }
    }
    // A float perturbation one ULP wide must still be caught: the adoption rule
    // is bit-identity, so the comparator must not carry a tolerance.
    let mut ulp = probe_metrics();
    ulp.max_altitude = f64::from_bits(base.max_altitude.to_bits() + 1);
    let du = diff_fields(&base, &ulp);
    println!("  one-ULP max_altitude       -> {du:?}");
    if !du.iter().any(|x| x == "max_altitude") {
        println!("  FAIL — a one-ULP float difference was not reported");
        failures += 1;
    }

    println!("\nselftest failures: {failures}");
    failures
}

/// A neutral Metrics value for the comparator controls.
fn probe_metrics() -> Metrics {
    Metrics {
        duration_secs: 1299.7,
        total_distance: 1234.5,
        max_altitude: 23.0,
        max_speed: 6.25,
        home_lat: Some(44.05),
        home_lon: Some(-123.09),
        point_count: 12998,
        product_type: "Unknown(178)".into(),
        frames_decoded: true,
        frame_count: 12998,
        gps_track_digest: "abc123".into(),
        battery_sn: "SN123".into(),
        drone_model: Some("Matrice 4TD".into()),
        aircraft_name: "Matrice 4TD".into(),
        aircraft_sn: "1581".into(),
        rc_sn: "RC1".into(),
        camera_sn: "CAM1".into(),
        log_version: 14,
        start_time: "2026-01-01T00:00:00+00:00".into(),
        header_duration_raw: 1299.7,
        header_distance: 0.122,
        header_max_height: 23.0,
        header_max_hspeed: 6.139,
        dropped_segments: 0,
        keychain_source: "cache".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The comparator must never report a difference between a value and itself.
    #[test]
    fn identical_metrics_report_no_difference() {
        assert!(diff_fields(&probe_metrics(), &probe_metrics()).is_empty());
    }

    // …and must report every field that actually moved. Without this the
    // harness's headline result ("nothing differed") is unfalsifiable.
    #[test]
    fn every_compared_field_is_actually_compared() {
        let base = probe_metrics();
        macro_rules! moved {
            ($field:ident = $v:expr, $name:literal) => {{
                let mut m = probe_metrics();
                m.$field = $v;
                let d = diff_fields(&base, &m);
                assert!(
                    d.iter().any(|x| x == $name),
                    "{} moved but diff_fields returned {:?}",
                    $name,
                    d
                );
            }};
        }
        moved!(duration_secs = 1.0, "duration_secs");
        moved!(total_distance = 1.0, "total_distance");
        moved!(max_altitude = 1.0, "max_altitude");
        moved!(max_speed = 1.0, "max_speed");
        moved!(home_lat = None, "home_lat");
        moved!(home_lon = None, "home_lon");
        moved!(point_count = 1, "point_count");
        moved!(product_type = "X".into(), "product_type");
        moved!(frames_decoded = false, "frames_decoded");
        moved!(frame_count = 1, "frame_count");
        moved!(gps_track_digest = "X".into(), "gps_track_digest");
        moved!(battery_sn = "X".into(), "battery_sn");
        moved!(drone_model = None, "drone_model");
        moved!(aircraft_name = "X".into(), "aircraft_name");
        moved!(aircraft_sn = "X".into(), "aircraft_sn");
        moved!(rc_sn = "X".into(), "rc_sn");
        moved!(camera_sn = "X".into(), "camera_sn");
        moved!(log_version = 13, "log_version");
        moved!(start_time = "X".into(), "start_time");
        moved!(header_duration_raw = 1.0, "header_duration_raw");
        moved!(header_distance = 1.0, "header_distance");
        moved!(header_max_height = 1.0, "header_max_height");
        moved!(header_max_hspeed = 1.0, "header_max_hspeed");
        moved!(dropped_segments = 9, "dropped_segments");
    }

    // The adoption rule is bit-identity. A tolerance-based comparison would
    // silently absorb sub-epsilon drift, which is exactly the class of change
    // a crate bump is most likely to produce.
    #[test]
    fn float_comparison_is_bitwise_not_tolerant() {
        let base = probe_metrics();
        let mut one_ulp = probe_metrics();
        one_ulp.total_distance = f64::from_bits(base.total_distance.to_bits() + 1);
        assert_eq!(
            diff_fields(&base, &one_ulp),
            vec!["total_distance".to_string()]
        );
    }

    // `keychain_source` is provenance, not a measurement — comparing it would
    // flag every log whose keychain came from cache in one arm.
    #[test]
    fn provenance_field_is_not_compared() {
        let base = probe_metrics();
        let mut other = probe_metrics();
        other.keychain_source = "fetched".into();
        assert!(diff_fields(&base, &other).is_empty());
    }

    // The Inspire-1 BCD decode is the candidate's entire behavioural delta.
    #[test]
    fn candidate_changes_only_inspire1_battery_sn() {
        use djilog_cand::layout::details::{parse_battery_sn, ProductType};
        let raw: Vec<u8> = vec![0x30, 0x39, 0x38, 0x37, 0x36, 0x35, 0x34, 0x33, 0x32, 0x31];
        let plain = String::from_utf8_lossy(&raw).trim_end_matches('\0').to_string();
        assert_ne!(parse_battery_sn(ProductType::Inspire1, raw.clone()), plain);
        assert_ne!(parse_battery_sn(ProductType::Inspire1Pro, raw.clone()), plain);
        assert_ne!(parse_battery_sn(ProductType::Inspire1RAW, raw.clone()), plain);
        // Every airframe the fleet actually operates.
        for pt in [
            ProductType::Avata2,
            ProductType::Matrice30,
            ProductType::Mavic3Pro,
            ProductType::FPV,
            ProductType::Unknown(178),
            ProductType::Unknown(137),
            ProductType::Unknown(139),
        ] {
            assert_eq!(parse_battery_sn(pt, raw.clone()), plain, "{pt:?} moved");
        }
    }

    // The gate is production's file, included verbatim — assert it is the real
    // one and not an accidental stub.
    #[test]
    fn production_gate_is_the_one_included() {
        assert!(gate::segment_ok(100.0, Some(10.0)));
        assert!(!gate::segment_ok(10_000.0, Some(1.0)));
        assert!(!gate::segment_ok(600.0, None));
        assert_eq!(gate::sanity_bound_header(1000.0, Some(100.0)), 100.0);
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mut corpora: Vec<(String, PathBuf)> = Vec::new();
    let mut out_path = PathBuf::from("p-eval-results.jsonl");
    let mut cache_dir = PathBuf::from("keychain-cache");
    let mut limit: Option<usize> = None;

    while let Some(a) = args.next() {
        match a.as_str() {
            "--selftest" => std::process::exit(selftest()),
            "--corpus" => {
                let spec = args.next().expect("--corpus NAME=PATH");
                let (n, p) = spec.split_once('=').expect("--corpus NAME=PATH");
                corpora.push((n.to_string(), PathBuf::from(p)));
            }
            "--out" => out_path = PathBuf::from(args.next().unwrap()),
            "--cache" => cache_dir = PathBuf::from(args.next().unwrap()),
            "--limit" => limit = Some(args.next().unwrap().parse().unwrap()),
            other => panic!("unknown arg {other}"),
        }
    }
    assert!(!corpora.is_empty(), "at least one --corpus NAME=PATH required");
    fs::create_dir_all(&cache_dir).expect("cache dir");

    // The key is read from the environment and never written anywhere: not to
    // the cache, not to the results file, not to stdout.
    let api_key = std::env::var("DJI_API_KEY")
        .ok()
        .filter(|k| !k.trim().is_empty());
    eprintln!(
        "p-eval: pinned=registry 0.5.7  candidate=git 88fcfc96  dji_key={}",
        if api_key.is_some() { "present" } else { "ABSENT" }
    );

    let mut out = fs::File::create(&out_path).expect("out file");
    let mut stats: BTreeMap<String, usize> = BTreeMap::new();
    let mut field_hits: BTreeMap<String, usize> = BTreeMap::new();
    let mut n = 0usize;

    for (cname, dir) in &corpora {
        let mut files: Vec<PathBuf> = fs::read_dir(dir)
            .unwrap_or_else(|e| panic!("read {dir:?}: {e}"))
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().map(|x| x == "txt").unwrap_or(false))
            .collect();
        files.sort();
        for path in files {
            if let Some(l) = limit {
                if n >= l {
                    break;
                }
            }
            let bytes = match fs::read(&path) {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("  read {path:?}: {e}");
                    continue;
                }
            };
            let hash = hex::encode(Sha256::digest(&bytes));
            let (kc, kc_src) = keychain_for(&bytes, &hash, &cache_dir, api_key.as_deref());

            let p = read_pinned(&bytes, kc.as_deref(), &kc_src);
            let c = read_cand(&bytes, kc.as_deref(), &kc_src);

            let (pm, pe) = match p {
                Ok(r) => (Some(r), None),
                Err(e) => (None, Some(e)),
            };
            let (cm, ce) = match c {
                Ok(r) => (Some(r), None),
                Err(e) => (None, Some(e)),
            };

            let mut differing = Vec::new();
            let mut first_div = None;
            if let (Some(a), Some(b)) = (&pm, &cm) {
                differing = diff_fields(&a.m, &b.m);
                if a.m.gps_track_digest != b.m.gps_track_digest {
                    first_div = a
                        .track
                        .iter()
                        .zip(b.track.iter())
                        .position(|(x, y)| x != y)
                        .or(Some(a.track.len().min(b.track.len())));
                }
            }

            *stats
                .entry(if differing.is_empty() {
                    "identical".into()
                } else {
                    "differing".into()
                })
                .or_insert(0) += 1;
            *stats.entry(format!("keychain:{kc_src}")).or_insert(0) += 1;
            if pe.is_some() || ce.is_some() {
                *stats.entry("parse_error".into()).or_insert(0) += 1;
            }
            for f in &differing {
                *field_hits.entry(f.clone()).or_insert(0) += 1;
            }

            let rec = FileResult {
                hash: hash.clone(),
                file: path.file_name().unwrap().to_string_lossy().to_string(),
                corpus: cname.clone(),
                size_bytes: bytes.len() as u64,
                pinned: pm.map(|r| r.m),
                cand: cm.map(|r| r.m),
                pinned_error: pe,
                cand_error: ce,
                differing_fields: differing,
                first_track_divergence: first_div,
            };
            writeln!(out, "{}", serde_json::to_string(&rec).unwrap()).unwrap();
            n += 1;
            if n % 25 == 0 {
                eprintln!("  {n} logs processed");
            }
        }
    }

    eprintln!("\n=== p-eval summary ({n} logs) ===");
    for (k, v) in &stats {
        eprintln!("  {k:24} {v}");
    }
    if field_hits.is_empty() {
        eprintln!("  NO FIELD DIFFERED on any log");
    } else {
        eprintln!("  differing fields:");
        for (k, v) in &field_hits {
            eprintln!("    {k:24} {v}");
        }
    }
}
