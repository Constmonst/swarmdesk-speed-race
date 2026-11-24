//! First occurrence of a needle in a haystack.
//! Input: the needle length m as 4 little-endian bytes, then m needle bytes, then the
//! haystack. Output: the index of the first occurrence as 4 little-endian bytes, or
//! 0xFFFFFFFF if there is none. An empty needle gives 0.
//!
//! The inputs are made so that only searching gives the answer. Needles that are there and
//! needles that are not have the same lengths and are made of the same bytes, and a needle
//! that is there starts at a place drawn evenly over the whole haystack.

use super::{Case, Target};
use crate::rng::Rng;

pub const TARGET: Target = Target {
    name: "find_substring",
    summary: "index of the first occurrence of a needle in a haystack, or 0xFFFFFFFF",
    reference,
    generate,
    out_cap: |_| 4,
};

fn reference(input: &[u8]) -> Vec<u8> {
    let m = u32::from_le_bytes([input[0], input[1], input[2], input[3]]) as usize;
    let needle = &input[4..4 + m];
    let haystack = &input[4 + m..];
    let mut found = u32::MAX;
    if m <= haystack.len() {
        for start in 0..=haystack.len() - m {
            if &haystack[start..start + m] == needle {
                found = start as u32;
                break;
            }
        }
    }
    found.to_le_bytes().to_vec()
}

fn build(needle: &[u8], haystack: &[u8]) -> Vec<u8> {
    let mut input = (needle.len() as u32).to_le_bytes().to_vec();
    input.extend_from_slice(needle);
    input.extend_from_slice(haystack);
    input
}

fn case(class: &'static str, needle: &[u8], haystack: &[u8]) -> Case {
    Case {
        class,
        input: build(needle, haystack),
    }
}

/// Inputs per run besides the edge cases. About half hold their needle, so a file that
/// leaves part of a haystack unsearched misses a needle in that part many times over the
/// runs of one verdict.
const CASES: usize = 400;
const MAX_HAY: usize = 8192;
const SMALL_HAY: usize = 64;

/// What a haystack is made of.
struct Kind {
    /// The byte values that occur.
    letters: Vec<u8>,
    /// True when the letters repeat in order, false when each byte is drawn from them.
    repeats: bool,
}

fn kind(rng: &mut Rng) -> Kind {
    let (count, repeats) = match rng.below(4) {
        0 => (256, false),
        1 => (rng.range(2, 4), false),
        2 => (26, false),
        _ => (rng.range(1, 4), true),
    };
    let letters = if count == 256 {
        (0..=255).collect()
    } else {
        rng.bytes(count)
    };
    Kind { letters, repeats }
}

fn draw(rng: &mut Rng, kind: &Kind, len: usize) -> Vec<u8> {
    let count = kind.letters.len();
    let at = |i: usize| kind.letters[i % count];
    if kind.repeats {
        return (0..len).map(at).collect();
    }
    (0..len)
        .map(|_| at(rng.below(count as u64) as usize))
        .collect()
}

/// A haystack, and the places where a repeating one was broken by a different byte. Without
/// such breaks every needle in a repeating haystack would be found within the first bytes.
fn haystack(rng: &mut Rng, kind: &Kind, len: usize) -> (Vec<u8>, Vec<usize>) {
    let mut hay = draw(rng, kind, len);
    let mut breaks = Vec::new();
    if kind.repeats && len > 0 {
        for _ in 0..rng.range(0, 3) {
            let at = rng.below(len as u64) as usize;
            hay[at] = rng.byte();
            breaks.push(at);
        }
    }
    (hay, breaks)
}

/// A byte of the same kind as the haystack's that differs from `old`.
fn other_letter(rng: &mut Rng, kind: &Kind, old: u8) -> u8 {
    let count = kind.letters.len();
    let from = rng.below(count as u64) as usize;
    let found = (0..count).map(|i| kind.letters[(from + i) % count]);
    found.into_iter().find(|&b| b != old).unwrap_or(old ^ 0x55)
}

/// A needle for a haystack that is not empty, and how it was made. Two in four are cut out
/// of the haystack, one in four is cut out and changed in one place, one in four is drawn
/// afresh from the same bytes. The reference decides whether and where it occurs.
fn needle(rng: &mut Rng, kind: &Kind, hay: &[u8], breaks: &[usize]) -> (&'static str, Vec<u8>) {
    let wanted = if rng.chance(70) {
        rng.range(1, 16)
    } else {
        rng.range(17, 64)
    };
    let how = rng.below(4);
    if how == 3 {
        return ("needle drawn afresh", draw(rng, kind, wanted));
    }
    let k = wanted.min(hay.len());
    let mut at = rng.range(0, hay.len() - k);
    if !breaks.is_empty() && rng.chance(60) {
        // End on a break, so the needle occurs there and nowhere before.
        let stop = breaks[rng.below(breaks.len() as u64) as usize];
        at = (stop + 1).saturating_sub(k).min(hay.len() - k);
    }
    let mut needle = hay[at..at + k].to_vec();
    if how < 2 {
        return ("needle cut out of the haystack", needle);
    }
    let change = rng.below(k as u64) as usize;
    needle[change] = other_letter(rng, kind, needle[change]);
    ("needle changed in one place", needle)
}

fn edge_cases(rng: &mut Rng) -> Vec<Case> {
    let (short_len, long_len, any_len) = (rng.range(0, 10), rng.range(11, 30), rng.range(1, 100));
    let short = rng.bytes(short_len);
    let long_needle = rng.bytes(long_len);
    let any = rng.bytes(any_len);
    let three = rng.bytes(3);
    let mut one_more = any.clone();
    one_more.push(rng.byte());
    vec![
        case("needle longer than haystack", &long_needle, &short),
        case("needle longer than haystack", &one_more, &any),
        case("needle is the whole haystack", &any, &any),
        case("empty needle", b"", &any),
        case("empty needle and haystack", b"", b""),
        case("empty haystack", &three, b""),
    ]
}

fn generate(rng: &mut Rng) -> Vec<Case> {
    let mut cases = edge_cases(rng);
    for _ in 0..CASES {
        let longest = if rng.chance(25) { SMALL_HAY } else { MAX_HAY };
        let len = rng.range(1, longest);
        let kind = kind(rng);
        let (hay, breaks) = haystack(rng, &kind, len);
        let (class, needle) = needle(rng, &kind, &hay, &breaks);
        cases.push(case(class, &needle, &hay));
    }
    cases
}

#[cfg(test)]
mod tests {
    use super::{build, generate, reference, MAX_HAY, TARGET};
    use crate::rng::Rng;

    fn find(needle: &[u8], haystack: &[u8]) -> u32 {
        let out = reference(&build(needle, haystack));
        u32::from_le_bytes([out[0], out[1], out[2], out[3]])
    }

    #[test]
    fn known_positions() {
        assert_eq!(find(b"abc", b"abcdef"), 0);
        assert_eq!(find(b"cd", b"abcdef"), 2);
        assert_eq!(find(b"ef", b"abcdef"), 4);
        assert_eq!(find(b"abab", b"aabababab"), 1);
        assert_eq!(find(b"aab", b"aaaaaab"), 4);
    }

    #[test]
    fn not_found_and_empty() {
        assert_eq!(find(b"xyz", b"abcdef"), u32::MAX);
        assert_eq!(find(b"abcdefg", b"abcdef"), u32::MAX);
        assert_eq!(find(b"a", b""), u32::MAX);
        assert_eq!(find(b"", b"abc"), 0);
        assert_eq!(find(b"", b""), 0);
    }

    #[test]
    fn generator_shape() {
        for seed in 0..5 {
            let cases = generate(&mut Rng::new(seed));
            assert!(cases.iter().map(|c| c.input.len()).sum::<usize>() < 2_500_000);
            for c in &cases {
                assert_eq!(reference(&c.input).len(), (TARGET.out_cap)(c.input.len()));
            }
        }
    }

    /// Needle length, haystack length and the answer, for inputs with a long haystack.
    fn long_cases(seeds: std::ops::Range<u64>) -> Vec<(usize, usize, u32)> {
        let mut out = Vec::new();
        for seed in seeds {
            for c in generate(&mut Rng::new(seed)) {
                let m = u32::from_le_bytes([c.input[0], c.input[1], c.input[2], c.input[3]]);
                let hay = c.input.len() - 4 - m as usize;
                let out_bytes = reference(&c.input);
                let at =
                    u32::from_le_bytes([out_bytes[0], out_bytes[1], out_bytes[2], out_bytes[3]]);
                if hay >= 1000 {
                    out.push((m as usize, hay, at));
                }
            }
        }
        out
    }

    /// Neither the needle's length nor the haystack's says whether the needle is there. Very
    /// short needles are left out: they occur by chance, and the answer is then the place.
    #[test]
    fn lengths_do_not_give_the_answer() {
        let cases = long_cases(0..2);
        for (lo, hi) in [(5, 16), (17, 64)] {
            let here = cases.iter().filter(|c| c.0 >= lo && c.0 <= hi);
            let found = here.clone().filter(|c| c.2 != u32::MAX).count();
            let absent = here.count() - found;
            assert!(
                found >= 20 && absent >= 20,
                "needle {lo}..{hi}: {found} {absent}"
            );
        }
        for quarter in 0..4 {
            let (lo, hi) = (quarter * MAX_HAY / 4, (quarter + 1) * MAX_HAY / 4);
            let here = cases.iter().filter(|c| c.1 > lo.max(999) && c.1 <= hi);
            let found = here.clone().filter(|c| c.2 != u32::MAX).count();
            let absent = here.count() - found;
            assert!(
                found >= 20 && absent >= 20,
                "haystack {lo}..{hi}: {found} {absent}"
            );
        }
    }

    /// A needle that is there can start anywhere, the last places included.
    #[test]
    fn matches_are_spread_over_the_whole_haystack() {
        let mut tenths = [0usize; 10];
        for (m, hay, at) in long_cases(0..4) {
            if at != u32::MAX {
                let last = (hay - m).max(1);
                tenths[(at as usize * 10 / last).min(9)] += 1;
            }
        }
        assert!(tenths.iter().all(|&n| n >= 20), "{tenths:?}");
    }
}
