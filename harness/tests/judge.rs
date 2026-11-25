//! End-to-end verdicts: each test builds real WebAssembly and runs it.

use sdcheck::judge::{judge, judge_runs_with_baseline, Report};
use sdcheck::targets;
use std::path::Path;

fn judge_crc32(candidate: &str) -> Report {
    let target = targets::find("crc32").unwrap();
    let work = sdcheck::work_dir();
    // A fixed baseline, not the live record holder: records move, tests must not.
    let holder = Path::new("tests/fixtures/crc32_baseline.rs");
    let report = judge(target, Path::new(candidate), holder, 11, &work);
    let _ = std::fs::remove_dir_all(&work);
    report
}

#[test]
fn a_faster_file_is_accepted() {
    let report = judge_crc32("tests/fixtures/crc32_fast.rs");
    assert_eq!((report.verdict, report.reason.as_str()), ("ACCEPTED", ""));
    assert_eq!(report.reason_code, "");
    assert!(report.candidate_wasm_bytes > 0);
    assert!(report.holder_wasm_bytes > 0);
    assert!(!report.class_fuel.is_empty());
    let (cand_sum, hold_sum) = report
        .class_fuel
        .values()
        .fold((0u64, 0u64), |acc, &(c, h)| (acc.0 + c, acc.1 + h));
    assert_eq!(cand_sum, report.candidate_fuel);
    assert_eq!(hold_sum, report.holder_fuel);
}

#[test]
fn a_file_does_not_beat_itself() {
    let report = judge_crc32("tests/fixtures/crc32_baseline.rs");
    assert_eq!(report.verdict, "REJECTED");
    assert_eq!(report.reason_code, "margin_too_small");
    assert_eq!(report.required_margin_bps, 300);
    assert!(report.reason.contains("percent less fuel"), "{}", report.reason);
}

#[test]
fn a_wrong_answer_is_rejected() {
    let report = judge_crc32("tests/fixtures/wrong.rs");
    assert_eq!(report.verdict, "REJECTED");
    assert_eq!(report.reason_code, "wrong_answer");
    assert!(report.reason.contains("wrong answer"), "{}", report.reason);
}

#[test]
fn an_endless_loop_runs_out_of_fuel() {
    let report = judge_crc32("tests/fixtures/endless.rs");
    assert_eq!(report.verdict, "REJECTED");
    assert_eq!(report.reason_code, "out_of_fuel");
    assert!(report.reason.contains("ran out of fuel"), "{}", report.reason);
}

#[test]
fn a_length_beyond_the_buffer_is_rejected() {
    let report = judge_crc32("tests/fixtures/too_long.rs");
    assert_eq!(report.verdict, "REJECTED");
    assert_eq!(report.reason_code, "wrong_answer");
    assert!(
        report.reason.contains("beyond the output buffer"),
        "{}",
        report.reason
    );
}

#[test]
fn a_file_that_does_not_compile_is_rejected() {
    let report = judge_crc32("tests/fixtures/broken.rs");
    assert_eq!(
        (report.verdict, report.reason.as_str(), report.reason_code),
        ("REJECTED", "does not compile", "compile_error")
    );
}

#[test]
fn a_file_with_a_comment_never_reaches_the_compiler() {
    let report = judge_crc32("tests/fixtures/comment.rs");
    assert_eq!(report.verdict, "REJECTED");
    assert_eq!(report.reason_code, "guard_rejected");
    assert!(report.reason.starts_with("guard: comments"), "{}", report.reason);
}

#[test]
fn every_record_holder_matches_its_reference() {
    for target in targets::all() {
        let source = Path::new("../targets").join(target.name).join("holder.rs");
        let work = sdcheck::work_dir();
        let report = judge(target, &source, &source, 3, &work);
        let _ = std::fs::remove_dir_all(&work);
        assert_eq!(
            report.verdict, "REJECTED",
            "{}: {}",
            target.name, report.reason
        );
        assert!(
            report.reason.contains("percent less fuel"),
            "{}: {}",
            target.name,
            report.reason
        );
    }
}

/// A file that is wrong for some inputs and happens to meet none of them on seed 6.
const POISON: &str = "tests/fixtures/crc32_poison.rs";

fn runs(candidate: &str, holder: &str, seeds: &[u64]) -> sdcheck::judge::Report {
    let target = targets::find("crc32").unwrap();
    let work = sdcheck::work_dir();
    let report = sdcheck::judge::judge_runs(
        target,
        Path::new(candidate),
        Path::new(holder),
        seeds,
        &work,
    );
    let _ = std::fs::remove_dir_all(&work);
    report
}

#[test]
fn one_lucky_set_of_inputs_is_not_enough() {
    let baseline = "tests/fixtures/crc32_baseline.rs";
    assert_eq!(runs(POISON, baseline, &[6]).verdict, "ACCEPTED");
    let confirmed = runs(POISON, baseline, &[6, 7, 8, 9, 10, 11]);
    assert_eq!(confirmed.verdict, "REJECTED");
    assert!(
        confirmed.reason.contains("wrong answer"),
        "{}",
        confirmed.reason
    );
    assert_eq!((confirmed.candidate_fuel, confirmed.holder_fuel), (0, 0));
}

#[test]
fn a_faulty_record_holder_blocks_nobody() {
    for candidate in [
        "tests/fixtures/crc32_fast.rs",
        "tests/fixtures/crc32_baseline.rs",
    ] {
        let report = runs(candidate, POISON, &[1, 2]);
        assert_eq!(report.verdict, "ACCEPTED", "{candidate}");
        assert!(report.holder_faulty);
        assert!(
            report.reason.contains("replaces a record holder"),
            "{}",
            report.reason
        );
    }
}

#[test]
fn a_wrong_file_does_not_replace_a_faulty_record_holder() {
    let report = runs("tests/fixtures/wrong.rs", POISON, &[1]);
    assert_eq!(report.verdict, "REJECTED");
}

#[test]
fn the_margin_is_taken_over_all_runs_together() {
    let report = runs(
        "tests/fixtures/crc32_fast.rs",
        "tests/fixtures/crc32_baseline.rs",
        &[11, 12, 13],
    );
    assert_eq!((report.verdict, report.runs), ("ACCEPTED", 3));
    assert!(report.cases > 60);
    assert!(!report.holder_faulty);
}

#[test]
fn a_trapping_submission_reports_trapped_reason_code() {
    let target = targets::find("crc32").unwrap();
    let work = sdcheck::work_dir();
    std::fs::create_dir_all(&work).unwrap();
    let trap_src = work.join("trap.rs");
    std::fs::write(
        &trap_src,
        "pub fn solve(_input: &[u8], _output: &mut [u8]) -> usize { unreachable!() }\n",
    )
    .unwrap();
    let report = judge(
        target,
        &trap_src,
        Path::new("tests/fixtures/crc32_baseline.rs"),
        11,
        &work,
    );
    let _ = std::fs::remove_dir_all(&work);
    assert_eq!(report.verdict, "REJECTED");
    assert_eq!(report.reason_code, "trapped");
    assert!(report.candidate_wasm_bytes > 0);
    assert!(report.holder_wasm_bytes > 0);
}

#[test]
fn judge_runs_with_baseline_tapers_required_margin() {
    let target = targets::find("crc32").unwrap();
    let work = sdcheck::work_dir();
    let baseline = Path::new("tests/fixtures/crc32_baseline.rs");
    let no_base = judge_runs_with_baseline(target, baseline, baseline, &[11], &work, None);
    assert_eq!(no_base.verdict, "REJECTED");
    assert_eq!(no_base.reason_code, "margin_too_small");
    assert_eq!(no_base.required_margin_bps, 300);

    let mid_base = judge_runs_with_baseline(
        target,
        baseline,
        baseline,
        &[11],
        &work,
        Some(no_base.holder_fuel * 2),
    );
    assert_eq!(mid_base.verdict, "REJECTED");
    assert_eq!(mid_base.reason_code, "margin_too_small");
    assert_eq!(mid_base.required_margin_bps, 150);
    assert!(mid_base.reason.contains("1.5 percent less fuel"), "{}", mid_base.reason);

    let low_base = judge_runs_with_baseline(
        target,
        baseline,
        baseline,
        &[11],
        &work,
        Some(no_base.holder_fuel * 4),
    );
    let _ = std::fs::remove_dir_all(&work);
    assert_eq!(low_base.verdict, "REJECTED");
    assert_eq!(low_base.reason_code, "margin_too_small");
    assert_eq!(low_base.required_margin_bps, 50);
    assert!(low_base.reason.contains("0.5 percent less fuel"), "{}", low_base.reason);
    let json = low_base.to_json();
    assert!(json.contains("\"required_margin_bps\":50"), "{json}");
    assert!(json.contains("\"reason_code\":\"margin_too_small\""), "{json}");
}
