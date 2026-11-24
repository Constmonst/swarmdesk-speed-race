# find_substring

Find the first occurrence of a needle in a haystack.

## Interface

- Input: 4 bytes holding the needle length m (little-endian), then m needle bytes, then the haystack (the rest of the input).
- Output: 4 bytes, little-endian: the index of the first occurrence of the needle in the haystack, or 0xFFFFFFFF if there is none. An empty needle gives 0. The function returns 4.
- Output buffer size: 4 bytes.

## Inputs

About 400 inputs in each run. Three in four haystacks are up to 8192 bytes long, one in four up to 64 bytes. Needles are 1 to 64 bytes long, most of them 16 or shorter. Needles that occur and needles that do not have the same lengths and are made of the same bytes, so only searching gives the answer.
- Haystacks: random bytes, a small alphabet of 2 to 4 byte values, an alphabet of 26 byte values, or 1 to 4 bytes repeating, broken in up to three places by a different byte.
- Needles: two in four are cut out of the haystack at a place drawn evenly over its whole length, one in four is cut out and then changed in one place, one in four is drawn afresh from the same bytes. In a repeating haystack many needles end on a break, so they occur there and nowhere before.
- A needle longer than the haystack, a needle that is the whole haystack, an empty needle, an empty haystack, and both empty.

## Rules

See [../../CONTRIBUTING.md](../../CONTRIBUTING.md).
