//! UTF-8 validity. Output: 1 byte, 1 if the input is valid UTF-8, else 0.
//!
//! The inputs are made so that only reading every byte gives the answer. Valid and invalid
//! inputs have the same lengths and the same kinds of text, and the one fault of an invalid
//! input sits at a place drawn evenly over its whole length, the last byte included.

use super::{Case, Target};
use crate::rng::Rng;

pub const TARGET: Target = Target {
    name: "utf8_valid",
    summary: "1 if the input is valid UTF-8, else 0",
    reference,
    generate,
    out_cap: |_| 1,
};

fn reference(input: &[u8]) -> Vec<u8> {
    vec![u8::from(std::str::from_utf8(input).is_ok())]
}

/// Inputs per run besides the empty one. Half are invalid, so a file that leaves part of an
/// input unread meets a fault in that part many times over the runs of one verdict.
const CASES: usize = 400;
const MAX_LEN: usize = 8192;
const SMALL_LEN: usize = 64;

/// Code points at the edges of the encoding. All are valid.
const EDGES: [u32; 10] = [
    0x00, 0x7F, 0x80, 0x7FF, 0x800, 0xD7FF, 0xE000, 0xFFFF, 0x1_0000, 0x10_FFFF,
];

const OVERLONG: [&[u8]; 6] = [
    &[0xC0, 0x80],
    &[0xC1, 0xBF],
    &[0xE0, 0x80, 0x80],
    &[0xE0, 0x9F, 0xBF],
    &[0xF0, 0x80, 0x80, 0x80],
    &[0xF0, 0x8F, 0xBF, 0xBF],
];
const SURROGATE: [&[u8]; 3] = [
    &[0xED, 0xA0, 0x80],
    &[0xED, 0xBF, 0xBF],
    &[0xED, 0xB0, 0x80],
];
const TOO_HIGH: [&[u8]; 3] = [
    &[0xF4, 0x90, 0x80, 0x80],
    &[0xF5, 0x80, 0x80, 0x80],
    &[0xF7, 0xBF, 0xBF, 0xBF],
];

/// How the characters of a text are chosen: 1 to 4 for characters of that many bytes, 0 for
/// an even mix of all four, 5 for ASCII with a few longer characters in between.
const WIDTHS: [usize; 9] = [0, 0, 5, 5, 1, 1, 2, 3, 4];

/// Appends one random character that takes `width` bytes.
fn push_char(rng: &mut Rng, out: &mut Vec<u8>, width: usize) {
    let (lo, hi) = [
        (0x00, 0x7F),
        (0x80, 0x7FF),
        (0x800, 0xFFFF),
        (0x1_0000, 0x10_FFFF),
    ][width - 1];
    let mut buf = [0u8; 4];
    loop {
        // The surrogate range is not a character; draw again.
        if let Some(c) = char::from_u32(rng.range(lo, hi) as u32) {
            out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            return;
        }
    }
}

/// Valid UTF-8 of `len` bytes or up to three more.
fn text(rng: &mut Rng, len: usize, width: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(len + 4);
    let mut buf = [0u8; 4];
    while out.len() < len {
        if rng.chance(2) {
            let edge = EDGES[rng.below(EDGES.len() as u64) as usize];
            let c = char::from_u32(edge).unwrap_or(' ');
            out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            continue;
        }
        let w = match width {
            0 => rng.range(1, 4),
            5 if rng.chance(90) => 1,
            5 => rng.range(2, 4),
            w => w,
        };
        push_char(rng, &mut out, w);
    }
    out
}

fn pick(rng: &mut Rng, list: &[&[u8]]) -> Vec<u8> {
    list[rng.below(list.len() as u64) as usize].to_vec()
}

/// Bytes that are invalid wherever they stand between two characters, and their name.
fn fault(rng: &mut Rng) -> (&'static str, Vec<u8>) {
    match rng.below(6) {
        0 => ("stray continuation byte", vec![rng.range(0x80, 0xBF) as u8]),
        1 => (
            "byte that starts nothing",
            vec![rng.range(0xF8, 0xFF) as u8],
        ),
        2 => ("overlong encoding", pick(rng, &OVERLONG)),
        3 => ("surrogate", pick(rng, &SURROGATE)),
        4 => ("above U+10FFFF", pick(rng, &TOO_HIGH)),
        _ => {
            // A character of two to four bytes that loses one or more of its last bytes.
            let width = rng.range(2, 4);
            let mut bytes = Vec::new();
            push_char(rng, &mut bytes, width);
            bytes.truncate(rng.range(1, width - 1));
            ("sequence cut short", bytes)
        }
    }
}

/// Text of about `len` bytes with exactly one fault, at a place drawn evenly over the length.
fn invalid(rng: &mut Rng, len: usize, width: usize) -> Case {
    if rng.chance(15) {
        let mut input = text(rng, len.max(1), width);
        let at = rng.below(input.len() as u64) as usize;
        input[at] = rng.byte();
        if std::str::from_utf8(&input).is_ok() {
            input[at] = 0xFF;
        }
        let class = "one corrupted byte";
        return Case { class, input };
    }
    let at = rng.range(0, len);
    let (class, bad) = fault(rng);
    let mut input = text(rng, at, width);
    input.extend_from_slice(&bad);
    input.extend(text(rng, len - at, width));
    Case { class, input }
}

fn generate(rng: &mut Rng) -> Vec<Case> {
    let mut cases = vec![Case {
        class: "empty",
        input: Vec::new(),
    }];
    for _ in 0..CASES {
        let longest = if rng.chance(25) { SMALL_LEN } else { MAX_LEN };
        let len = rng.range(0, longest);
        let width = WIDTHS[rng.below(WIDTHS.len() as u64) as usize];
        cases.push(if rng.chance(50) {
            let class = "valid text";
            let input = text(rng, len, width);
            Case { class, input }
        } else {
            invalid(rng, len, width)
        });
    }
    cases
}

#[cfg(test)]
mod tests {
    use super::{generate, reference, EDGES, MAX_LEN, TARGET};
    use crate::rng::Rng;

    #[test]
    fn known_valid() {
        assert_eq!(reference(b""), [1]);
        assert_eq!(reference(b"plain ascii"), [1]);
        assert_eq!(reference("h\u{e9}llo \u{20AC} \u{1F600}".as_bytes()), [1]);
        assert_eq!(reference(&[0xF4, 0x8F, 0xBF, 0xBF]), [1]);
        assert_eq!(reference(&[0x00, 0x1F, 0x7F]), [1]);
    }

    #[test]
    fn known_invalid() {
        assert_eq!(reference(&[0xC0, 0x80]), [0]);
        assert_eq!(reference(&[0xE0, 0x9F, 0xBF]), [0]);
        assert_eq!(reference(&[0xF0, 0x8F, 0xBF, 0xBF]), [0]);
        assert_eq!(reference(&[0xED, 0xA0, 0x80]), [0]);
        assert_eq!(reference(&[0xF4, 0x90, 0x80, 0x80]), [0]);
        assert_eq!(reference(&[0xE2, 0x82]), [0]);
        assert_eq!(reference(&[0x80]), [0]);
    }

    #[test]
    fn classes_have_the_right_answer() {
        for seed in 0..10 {
            let cases = generate(&mut Rng::new(seed));
            assert!(cases.iter().map(|c| c.input.len()).sum::<usize>() < 2_500_000);
            let valid = cases.iter().filter(|c| reference(&c.input) == [1]).count();
            assert!(valid * 100 >= cases.len() * 40 && valid * 100 <= cases.len() * 60);
            for c in &cases {
                assert_eq!((TARGET.out_cap)(c.input.len()), 1);
                let expected = u8::from(matches!(c.class, "empty" | "valid text"));
                assert_eq!(reference(&c.input), [expected], "{}", c.class);
            }
        }
    }

    /// The length of an input says nothing about its answer: in every quarter of the length
    /// range there are plenty of valid and plenty of invalid inputs.
    #[test]
    fn every_length_band_has_both_answers() {
        let cases = generate(&mut Rng::new(7));
        for band in 0..4 {
            let (lo, hi) = (band * MAX_LEN / 4, (band + 1) * MAX_LEN / 4);
            let here = |c: &&super::Case| c.input.len() > lo.max(64) && c.input.len() <= hi;
            let all = cases.iter().filter(here).count();
            let valid = cases
                .iter()
                .filter(here)
                .filter(|c| reference(&c.input) == [1]);
            let valid = valid.count();
            assert!(
                valid >= 20 && all - valid >= 20,
                "band {band}: {valid} of {all}"
            );
        }
    }

    /// The fault of an invalid input is spread over the whole length, the end included.
    #[test]
    fn faults_are_spread_over_the_whole_input() {
        let mut tenths = [0usize; 10];
        for seed in 0..4 {
            for c in generate(&mut Rng::new(seed)) {
                let len = c.input.len();
                if len < 1000 || reference(&c.input) == [1] {
                    continue;
                }
                let good = std::str::from_utf8(&c.input).unwrap_err().valid_up_to();
                tenths[(good * 10 / len).min(9)] += 1;
            }
        }
        assert!(tenths.iter().all(|&n| n >= 20), "{tenths:?}");
    }

    #[test]
    fn edges_are_valid_characters() {
        assert!(EDGES.iter().all(|&code| char::from_u32(code).is_some()));
    }
}
