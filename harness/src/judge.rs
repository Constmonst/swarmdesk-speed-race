//! Decides one submission: guard, build, then run it and the record holder on
//! the same fresh inputs. The verdict comes only from what this process
//! measures; nothing a submission prints or returns besides its output counts.

use crate::build::{self, CompileError};
use crate::guard;
use crate::rng::Rng;
use crate::run::{self, Program};
use crate::targets::{Case, Target};
use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;
use wasmtime::Engine;

/// A submission must burn at least this much less fuel than the record holder when no baseline
/// taper applies.
pub const MARGIN_PERCENT: u64 = 3;
const HOLDER_FUEL_CAP: u64 = 2_000_000_000;

pub struct Report {
    pub verdict: &'static str,
    pub reason: String,
    pub reason_code: &'static str,
    pub target: &'static str,
    pub seed: u64,
    pub cases: usize,
    /// How many sets of inputs were run. The deciding check runs several, each from a new seed.
    pub runs: usize,
    pub candidate_fuel: u64,
    pub holder_fuel: u64,
    pub candidate_wasm_bytes: usize,
    pub holder_wasm_bytes: usize,
    pub required_margin_bps: u64,
    pub class_fuel: BTreeMap<&'static str, (u64, u64)>,
    /// The record holder gave a wrong answer on at least one input.
    pub holder_faulty: bool,
    /// Help for the author on their own machine: which input failed and how. Printed by
    /// the command line, never sent anywhere.
    pub detail: String,
}

impl Report {
    fn new(target: &'static str, seed: u64) -> Report {
        Report {
            verdict: "FAULT",
            reason: String::new(),
            reason_code: "",
            target,
            seed,
            cases: 0,
            runs: 0,
            candidate_fuel: 0,
            holder_fuel: 0,
            candidate_wasm_bytes: 0,
            holder_wasm_bytes: 0,
            required_margin_bps: MARGIN_PERCENT * 100,
            class_fuel: BTreeMap::new(),
            holder_faulty: false,
            detail: String::new(),
        }
    }

    fn end(
        mut self,
        verdict: &'static str,
        reason_code: &'static str,
        reason: impl Into<String>,
    ) -> Report {
        self.verdict = verdict;
        self.reason_code = reason_code;
        self.reason = reason.into();
        self
    }

    /// One line for a person: `ACCEPTED`, `REJECTED (<reason>)` or `FAULT (<reason>)`.
    pub fn line(&self) -> String {
        if self.reason.is_empty() {
            self.verdict.to_string()
        } else {
            format!("{} ({})", self.verdict, self.reason)
        }
    }

    pub fn to_json(&self) -> String {
        let reason = json_text(&self.reason);
        let reason_code = json_text(self.reason_code);
        let class_fuel: String = self
            .class_fuel
            .iter()
            .map(|(class, (cand, hold))| {
                format!(
                    "\"{}\":{{\"candidate\":{},\"holder\":{}}}",
                    json_text(class),
                    cand,
                    hold
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!(
            concat!(
                "{{\"verdict\":\"{}\",\"reason\":\"{}\",\"reason_code\":\"{}\",\"target\":\"{}\",",
                "\"seed\":\"{}\",\"cases\":{},\"runs\":{},\"candidate_fuel\":{},\"holder_fuel\":{},",
                "\"candidate_wasm_bytes\":{},\"holder_wasm_bytes\":{},\"holder_faulty\":{},",
                "\"margin_percent\":{},\"required_margin_bps\":{},\"class_fuel\":{{{}}}}}"
            ),
            self.verdict,
            reason,
            reason_code,
            self.target,
            self.seed,
            self.cases,
            self.runs,
            self.candidate_fuel,
            self.holder_fuel,
            self.candidate_wasm_bytes,
            self.holder_wasm_bytes,
            self.holder_faulty,
            MARGIN_PERCENT,
            self.required_margin_bps,
            class_fuel
        )
    }
}

/// `text` as the inside of a JSON string.
fn json_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// Judges `candidate` against `holder` for `target` with inputs from `seed`.
pub fn judge(
    target: &'static Target,
    candidate: &Path,
    holder: &Path,
    seed: u64,
    work: &Path,
) -> Report {
    judge_runs(target, candidate, holder, &[seed], work)
}

/// Judges on one set of inputs per seed. The candidate must answer every input of every set
/// correctly, and the margin is taken over all sets together, so one lucky set decides nothing.
pub fn judge_runs(
    target: &'static Target,
    candidate: &Path,
    holder: &Path,
    seeds: &[u64],
    work: &Path,
) -> Report {
    judge_runs_with_baseline(target, candidate, holder, seeds, work, None)
}

/// Judges on one set of inputs per seed, tapering the required win margin when `baseline_fuel`
/// shows the record holder has already been pushed far below the original baseline.
pub fn judge_runs_with_baseline(
    target: &'static Target,
    candidate: &Path,
    holder: &Path,
    seeds: &[u64],
    work: &Path,
    baseline_fuel: Option<u64>,
) -> Report {
    let mut report = Report::new(target.name, seeds.first().copied().unwrap_or(0));
    let source = match std::fs::read(candidate) {
        Ok(bytes) => bytes,
        Err(_) => return report.end("FAULT", "fault", "the submission could not be read"),
    };
    if let Err(reason) = guard::check(&source) {
        return report.end("REJECTED", "guard_rejected", reason);
    }
    let engine = match run::engine() {
        Ok(engine) => engine,
        Err(reason) => return report.end("FAULT", "fault", reason),
    };
    let (holder_program, holder_wasm_bytes) = match load(&engine, holder, work, "holder") {
        Ok(loaded) => loaded,
        Err(err) => return report.end("FAULT", "fault", format!("record holder: {err}")),
    };
    report.holder_wasm_bytes = holder_wasm_bytes;
    let (candidate_program, candidate_wasm_bytes) = match load(&engine, candidate, work, "candidate")
    {
        Ok(loaded) => loaded,
        Err(LoadError::Compile(CompileError::Infrastructure(msg))) => {
            return report.end("FAULT", "fault", msg);
        }
        Err(LoadError::Compile(err)) => {
            return report.end("REJECTED", err.reason_code(), err.to_string());
        }
        Err(LoadError::VetOrModule(msg)) => {
            return report.end("REJECTED", "compile_error", msg);
        }
    };
    report.candidate_wasm_bytes = candidate_wasm_bytes;
    for &seed in seeds {
        let cases = (target.generate)(&mut Rng::new(seed));
        let programs = (&candidate_program, &holder_program);
        if let Err((reason_code, reason)) =
            race(&mut report, target, &engine, programs, &cases, seed)
        {
            // A run that stops early has no meaningful totals.
            (report.candidate_fuel, report.holder_fuel) = (0, 0);
            report.class_fuel.clear();
            return report.end("REJECTED", reason_code, reason);
        }
        report.runs += 1;
    }
    decide(report, baseline_fuel)
}

/// The verdict for a candidate that answered every input correctly.
fn decide(mut report: Report, baseline_fuel: Option<u64>) -> Report {
    let margin_bps = required_margin_bps(report.holder_fuel, baseline_fuel);
    report.required_margin_bps = margin_bps;
    // A record holder that answers wrongly is no record. A correct file replaces it whatever
    // its fuel, so that a faulty record holder can never block its function.
    if report.holder_faulty {
        let why = "replaces a record holder that gave a wrong answer";
        return report.end("ACCEPTED", "", why);
    }
    if wins_bps(report.candidate_fuel, report.holder_fuel, margin_bps) {
        return report.end("ACCEPTED", "", "");
    }
    let pct = format_margin_percent(margin_bps);
    let why = format!("not at least {pct} percent less fuel than the record holder");
    report.end("REJECTED", "margin_too_small", why)
}

fn format_margin_percent(bps: u64) -> String {
    if bps % 100 == 0 {
        format!("{}", bps / 100)
    } else if bps % 10 == 0 {
        format!("{}.{}", bps / 100, (bps % 100) / 10)
    } else {
        format!("{}.{:02}", bps / 100, bps % 100)
    }
}

enum LoadError {
    Compile(CompileError),
    VetOrModule(String),
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Compile(err) => err.fmt(f),
            Self::VetOrModule(msg) => f.write_str(msg),
        }
    }
}

fn load(
    engine: &Engine,
    source: &Path,
    work: &Path,
    label: &str,
) -> Result<(Program, usize), LoadError> {
    let wasm = build::compile_checked(source, work, label).map_err(LoadError::Compile)?;
    let bytes = wasm.len();
    let program = Program::load(engine, &wasm).map_err(LoadError::VetOrModule)?;
    Ok((program, bytes))
}

/// Runs one set of inputs. `Err` holds `(reason_code, reason)` when the candidate is rejected.
fn race(
    report: &mut Report,
    target: &Target,
    engine: &Engine,
    (candidate, holder): (&Program, &Program),
    cases: &[Case],
    seed: u64,
) -> Result<(), (&'static str, String)> {
    report.cases += cases.len();
    for (index, case) in cases.iter().enumerate() {
        let expected = (target.reference)(&case.input);
        let cap = (target.out_cap)(case.input.len());
        let held = match run::run_case(engine, holder, &case.input, cap, HOLDER_FUEL_CAP) {
            Ok(outcome) if outcome.output == expected => Some(outcome.fuel),
            _ => None,
        };
        let fuel_cap = match held {
            Some(fuel) => fuel.saturating_mul(8).saturating_add(100_000),
            None => HOLDER_FUEL_CAP,
        };
        let failure = match run::run_case(engine, candidate, &case.input, cap, fuel_cap) {
            Ok(outcome) if outcome.output == expected => {
                // Fuel is compared only on inputs both files answered correctly.
                match held {
                    Some(fuel) => {
                        report.candidate_fuel += outcome.fuel;
                        report.holder_fuel += fuel;
                        let entry = report.class_fuel.entry(case.class).or_insert((0, 0));
                        entry.0 += outcome.fuel;
                        entry.1 += fuel;
                    }
                    None => report.holder_faulty = true,
                }
                continue;
            }
            Ok(outcome) => (
                "wrong_answer",
                "wrong answer".to_string(),
                mismatch(&expected, &outcome.output),
            ),
            Err(reason) => (run_reason_code(&reason), reason, String::new()),
        };
        report.detail = format!(
            "stopped at input {} of {} in run {} with seed {} ({}, {} bytes). {}",
            index + 1,
            cases.len(),
            report.runs + 1,
            seed,
            case.class,
            case.input.len(),
            failure.2
        );
        return Err((failure.0, format!("{}, input class: {}", failure.1, case.class)));
    }
    Ok(())
}

fn run_reason_code(reason: &str) -> &'static str {
    if reason == "ran out of fuel" {
        "out_of_fuel"
    } else if reason == "returned a length beyond the output buffer" {
        "wrong_answer"
    } else {
        "trapped"
    }
}

/// Says where two answers part ways, for an author debugging on their own machine.
fn mismatch(expected: &[u8], got: &[u8]) -> String {
    let shared = expected.len().min(got.len());
    let lengths = format!("expected {} bytes, got {}", expected.len(), got.len());
    match (0..shared).find(|&i| expected[i] != got[i]) {
        Some(at) => format!(
            "{lengths}; first difference at byte {at}: expected 0x{:02x}, got 0x{:02x}",
            expected[at], got[at]
        ),
        None => lengths,
    }
}

/// Required win margin in basis points (`100` bps = 1%). Tapers as `holder_fuel` drops
/// significantly below `baseline_fuel`, keeping late-stage optimizations reachable.
pub fn required_margin_bps(holder_fuel: u64, baseline_fuel: Option<u64>) -> u64 {
    let Some(base) = baseline_fuel.filter(|&b| b > 0) else {
        return MARGIN_PERCENT * 100;
    };
    let holder = u128::from(holder_fuel) * 100;
    let base = u128::from(base);
    if holder >= base * 60 {
        300
    } else if holder >= base * 35 {
        150
    } else {
        50
    }
}

/// True when the candidate burns at least `margin_bps` basis points less fuel than the holder.
pub fn wins_bps(candidate_fuel: u64, holder_fuel: u64, margin_bps: u64) -> bool {
    u128::from(candidate_fuel) * 10_000
        <= u128::from(holder_fuel) * u128::from(10_000u64.saturating_sub(margin_bps))
}

/// True when the candidate burns at least `MARGIN_PERCENT` less fuel.
pub fn wins(candidate_fuel: u64, holder_fuel: u64) -> bool {
    wins_bps(candidate_fuel, holder_fuel, MARGIN_PERCENT * 100)
}

#[cfg(test)]
mod tests {
    use super::{mismatch, required_margin_bps, wins, wins_bps, Report};

    #[test]
    fn a_mismatch_names_the_first_differing_byte() {
        assert_eq!(
            mismatch(&[1, 2, 3, 4], &[1, 2, 9, 4]),
            "expected 4 bytes, got 4; first difference at byte 2: expected 0x03, got 0x09"
        );
        assert_eq!(mismatch(&[1, 2, 3], &[1, 2]), "expected 3 bytes, got 2");
    }

    #[test]
    fn a_reason_with_any_character_is_still_json() {
        assert_eq!(super::json_text("plain"), "plain");
        assert_eq!(super::json_text("a \"b\" \\ c"), "a \\\"b\\\" \\\\ c");
        assert_eq!(
            super::json_text("line\nnext\ttab"),
            "line\\u000anext\\u0009tab"
        );
    }

    #[test]
    fn margin_is_exact() {
        assert!(wins(97, 100));
        assert!(!wins(98, 100));
        assert!(!wins(100, 100));
        assert!(wins(0, 100));
    }

    #[test]
    fn tapered_margin_tiers() {
        assert_eq!(required_margin_bps(500, None), 300);
        assert_eq!(required_margin_bps(500, Some(0)), 300);
        assert_eq!(required_margin_bps(600, Some(1_000)), 300);
        assert_eq!(required_margin_bps(599, Some(1_000)), 150);
        assert_eq!(required_margin_bps(350, Some(1_000)), 150);
        assert_eq!(required_margin_bps(349, Some(1_000)), 50);
        assert_eq!(required_margin_bps(0, Some(1_000)), 50);
    }

    #[test]
    fn wins_bps_handles_tapered_margins_and_large_fuel() {
        assert!(wins_bps(985, 1_000, 150));
        assert!(!wins_bps(986, 1_000, 150));
        assert!(wins_bps(995, 1_000, 50));
        assert!(!wins_bps(996, 1_000, 50));
        assert!(wins_bps(u64::MAX - 10_000, u64::MAX, 0));
    }

    #[test]
    fn report_to_json_includes_new_telemetry_fields() {
        let mut report = Report::new("crc32", 42);
        report.candidate_fuel = 9_850;
        report.holder_fuel = 10_000;
        report.candidate_wasm_bytes = 512;
        report.holder_wasm_bytes = 768;
        report.class_fuel.insert("empty", (10, 20));
        report.class_fuel.insert("random", (9_840, 9_980));
        let report = super::decide(report, Some(20_000));
        let json = report.to_json();
        assert!(json.contains("\"verdict\":\"ACCEPTED\""), "{json}");
        assert!(json.contains("\"reason_code\":\"\""), "{json}");
        assert!(json.contains("\"candidate_wasm_bytes\":512"), "{json}");
        assert!(json.contains("\"holder_wasm_bytes\":768"), "{json}");
        assert!(json.contains("\"required_margin_bps\":150"), "{json}");
        assert!(
            json.contains(
                "\"class_fuel\":{\"empty\":{\"candidate\":10,\"holder\":20},\"random\":{\"candidate\":9840,\"holder\":9980}}"
            ),
            "{json}"
        );
    }
}
