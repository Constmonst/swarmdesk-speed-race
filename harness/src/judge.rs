//! Decides one submission: guard, build, then run it and the record holder on
//! the same fresh inputs. The verdict comes only from what this process
//! measures; nothing a submission prints or returns besides its output counts.

use crate::build::{self, CompileError};
use crate::guard;
use crate::margin::format_margin_percent;
use crate::rng::Rng;
use crate::run::{self, Program};
use crate::targets::{Case, Target};
use std::fmt;
use std::path::Path;
use wasmtime::Engine;

pub use crate::margin::{required_margin_bps, wins, wins_bps, MARGIN_PERCENT};
pub use crate::report::Report;

const HOLDER_FUEL_CAP: u64 = 2_000_000_000;

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
    let (candidate_program, candidate_wasm_bytes) =
        match load(&engine, candidate, work, "candidate") {
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
        return Err((
            failure.0,
            format!("{}, input class: {}", failure.1, case.class),
        ));
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

#[cfg(test)]
mod tests {
    use super::{mismatch, Report};

    #[test]
    fn a_mismatch_names_the_first_differing_byte() {
        assert_eq!(
            mismatch(&[1, 2, 3, 4], &[1, 2, 9, 4]),
            "expected 4 bytes, got 4; first difference at byte 2: expected 0x03, got 0x09"
        );
        assert_eq!(mismatch(&[1, 2, 3], &[1, 2]), "expected 3 bytes, got 2");
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
