//! The report one judging produces, and its JSON form for the site.

use crate::margin::MARGIN_PERCENT;
use std::collections::BTreeMap;

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
    pub(crate) fn new(target: &'static str, seed: u64) -> Report {
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

    pub(crate) fn end(
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

#[cfg(test)]
mod tests {
    use super::json_text;

    #[test]
    fn a_reason_with_any_character_is_still_json() {
        assert_eq!(json_text("plain"), "plain");
        assert_eq!(json_text("a \"b\" \\ c"), "a \\\"b\\\" \\\\ c");
        assert_eq!(json_text("line\nnext\ttab"), "line\\u000anext\\u0009tab");
    }
}
