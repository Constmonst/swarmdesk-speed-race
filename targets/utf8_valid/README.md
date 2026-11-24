# utf8_valid

Decide whether the input is valid UTF-8.

## Interface

- Input: any bytes.
- Output: 1 byte, 1 if the input is valid UTF-8, else 0. The function returns 1.
- Valid means what `std::str::from_utf8` accepts: no overlong encodings, no surrogates (U+D800 to U+DFFF), no code points above U+10FFFF, no truncated sequences.
- Output buffer size: 1 byte.

## Inputs

About 400 inputs in each run, about half of them valid. Three in four are up to 8192 bytes long, one in four up to 64 bytes. Valid and invalid inputs have the same lengths and are the same kinds of text, so only reading every byte gives the answer.
- Text: ASCII (all of 00 to 7F), ASCII with a few longer characters, 2-, 3- or 4-byte characters only, or a mix of all four. The characters at the edges of the encoding occur (U+007F, U+0080, U+07FF, U+0800, U+D7FF, U+E000, U+FFFF, U+10000, U+10FFFF).
- An invalid input is such text with one fault at a place drawn evenly over its whole length, the end included: a stray continuation byte, a byte that starts nothing (F8 to FF), an overlong encoding (such as C0 80 or E0 9F BF), a surrogate (ED A0 80 and up), a value above U+10FFFF (F4 90 80 80 and up), a sequence that lost its last bytes, or one corrupted byte.

## Rules

See [../../CONTRIBUTING.md](../../CONTRIBUTING.md).
