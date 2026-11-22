//! `LZWDecode`, decoded by the engine so that a container's output is bounded while it is produced.
//!
//! The filter is read in the reference's shape — pdf-lib's `LZWStream`, which `PARSER.md` §3.15
//! states and witnesses — and the bound is `container_decode_max_bytes`, whose meaning and limits
//! `VOCABULARY.md` states. This module is that decoder and nothing else: the reference's answer,
//! counted as each code produces it, and `None` for a stream past the ceiling, which the walk then
//! carries as the bytes it states.

/// The codes one block of the reference's decoder holds. It decodes a block at a time and pads the
/// block a stream ends inside with zero codes, so this number is part of the answer, not a buffer size.
const CODES: usize = 512;

/// The table's size: a code is at most twelve bits wide and the reference's decoder never grows the
/// table past this.
const TABLE: usize = 4096;

/// The literal codes' ceiling: the first code that is not a literal byte.
const LITERALS: u16 = 256;

/// The code that clears the table.
const CLEAR: u16 = LITERALS;

/// The code that ends the data.
const EOD: u16 = LITERALS + 1;

/// The first code the table states, past the literals and the two codes the format reserves.
const FIRST: u16 = LITERALS + 2;

/// The width the table's first code needs, which is the width the reference's decoder starts at.
const MIN_BITS: u32 = FIRST.bit_width();

/// The width no code passes.
const MAX_BITS: u32 = 12;

/// The stream's decoded bytes, or `None` when the next code would pass `max`. `early_change` is the
/// `/EarlyChange` the stream states, or the format's default.
pub(super) fn decode(data: &[u8], early_change: f64, max: u64) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    decode_into(data, early_change, max, &mut out).then_some(out)
}

/// [`decode`] into a buffer the caller holds, so a witness can see how much was produced before the
/// ceiling stopped it. `false` is a stream past `max`; `out` then holds what the codes before it
/// produced, which is never more than `max`.
fn decode_into(data: &[u8], early_change: f64, max: u64, out: &mut Vec<u8>) -> bool {
    let mut bits = Bits::new(data);
    // The table: a literal's value is itself, and an entry added later is the sequence before it with
    // one byte appended. `previous` is the entry each one was grown from, so an entry's sequence is
    // rebuilt by following it back.
    let mut values = [0_u8; TABLE];
    let mut lengths = [0_u16; TABLE];
    let mut previous = [0_u16; TABLE];
    for (index, (value, length)) in values.iter_mut().zip(lengths.iter_mut()).enumerate() {
        *value = u8::try_from(index).unwrap_or_default();
        // A literal states one byte; an entry no code has added states nothing, and the reference
        // reads that as a length of zero rather than as one.
        *length = u16::from(index < usize::from(LITERALS));
    }
    let mut next = usize::from(FIRST);
    let mut code_length = MIN_BITS;
    let mut sequence = [0_u8; TABLE];
    let mut length = 0_usize;
    let mut last = 0_u16;
    // The reference decodes a block at a time and stops when its input ends inside one, so the
    // iteration count is what the padding is measured from.
    loop {
        for code_index in 0..CODES {
            let Some(code) = bits.read(code_length) else {
                // The bytes ran out: the reference emits one zero code per code the block had left
                // rather than reporting the cut, and then reports its end of data.
                let pad = CODES - code_index;
                if !room(out, pad, max) {
                    return false;
                }
                out.resize(out.len() + pad, 0);
                return true;
            };
            let had_previous = length > 0;
            match code {
                code if code < LITERALS => {
                    sequence[0] = u8::try_from(code).unwrap_or_default();
                    length = 1;
                }
                CLEAR => {
                    next = usize::from(FIRST);
                    code_length = MIN_BITS;
                    length = 0;
                    continue;
                }
                EOD => return true,
                code => {
                    if usize::from(code) < next {
                        length = usize::from(lengths[usize::from(code)]);
                        // A clear code states the table from its first entry again but keeps the
                        // lengths the previous table grew, so an entry can state more bytes than the
                        // table holds. The reference walks the whole chain, drops the writes past its
                        // fixed array and states zeros where they would have been.
                        let mut entry = code;
                        for index in (0..length).rev() {
                            if index < TABLE {
                                sequence[index] = values[usize::from(entry)];
                            }
                            entry = previous[usize::from(entry)];
                        }
                    } else {
                        // A code the table does not hold — the format's one-step repeat, which the
                        // reference takes for every code past the table rather than refusing it.
                        if length < TABLE {
                            sequence[length] = sequence[0];
                        }
                        length += 1;
                    }
                }
            }
            if had_previous {
                if next < TABLE {
                    previous[next] = last;
                    // The reference states the length into a fixed array of sixteen-bit entries, which
                    // wraps rather than saturating.
                    lengths[next] = lengths[usize::from(last)].wrapping_add(1);
                    values[next] = sequence[0];
                }
                next += 1;
                code_length = grown(next, early_change, code_length);
            }
            last = code;
            // A sequence grown past the table states zeros there, as the reference's fixed array does.
            if !room(out, length, max) {
                return false;
            }
            let held = length.min(TABLE);
            out.extend_from_slice(&sequence[..held]);
            out.resize(out.len() + (length - held), 0);
        }
    }
}

/// The modulus the reference's `ToInt32` coercion wraps at: one past the largest 32-bit integer.
const WRAP: f64 = u32::MAX as f64 + 1.0;

/// The width the reference reads its next code at, once the table gains an entry: it recomputes the
/// width only when the table's size plus `/EarlyChange` is a power of two, so the entry moves the
/// crossing point rather than the formula, and a width no code passes is twelve. `previous` is the
/// width the last crossing stated, which a sum that is not a power of two leaves in place.
#[expect(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the index the table states is a few thousand and exact in a double; the sum it is added \
              to is the number the file states, and truncating it is the reference's own coercion \
              rather than a panic"
)]
pub(crate) fn grown(next: usize, early_change: f64, previous: u32) -> u32 {
    let sum = next as f64 + early_change;
    // The reference's power-of-two test runs on the sum `ToInt32` coerces, twice: the entry, and the
    // sum minus one computed in double arithmetic — which is not the entry minus one once the sum is
    // past two to the fifty-third, where the subtraction itself rounds. A sum that is not a number
    // wraps to zero both times, as `ToInt32` answers it.
    let whole = sum.trunc().rem_euclid(WRAP) as u32;
    let below = (sum - 1.0).trunc().rem_euclid(WRAP) as u32;
    if whole & below != 0 {
        return previous;
    }
    // `Math.min` answers a sum with no logarithm with no width at all, where `f64::min` would answer the
    // bound, so the logarithm is taken only of a sum that has one. A width below zero is read as zero
    // here, where the reference's coercion keeps the negative width; no writer states such a sum
    // (`PARSER.md` §3.15).
    if sum > 0.0 {
        (sum.log2() + 1.0).min(f64::from(MAX_BITS)) as u32
    } else {
        0
    }
}

/// Reserves room for `length` more bytes, refusing a total past `max`. The buffer grows by doubling
/// but never reserves past the ceiling, which a plain doubling would: at the ceiling it would hold
/// twice it.
fn room(out: &mut Vec<u8>, length: usize, max: u64) -> bool {
    let Some(end) = out.len().checked_add(length) else {
        return false;
    };
    if end as u64 > max {
        return false;
    }
    if out.capacity() < end {
        let ceiling = usize::try_from(max).unwrap_or(usize::MAX);
        let grown = out.capacity().saturating_mul(2).max(end).min(ceiling);
        out.reserve_exact(grown - out.len());
    }
    true
}

/// The codes the stream states, most significant bit first, and `None` for a code the bytes do not
/// hold — the reference's reader answers its end of data the same way, without a shorter code.
struct Bits<'a> {
    data: &'a [u8],
    at: usize,
    cached: u32,
    held: u32,
}

impl<'a> Bits<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            at: 0,
            cached: 0,
            held: 0,
        }
    }

    /// The next `width`-bit code. The width never passes twelve, so the mask keeps the code within the
    /// type the table is indexed with.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "the mask states at most twelve bits, which a `u16` holds"
    )]
    fn read(&mut self, width: u32) -> Option<u16> {
        while self.held < width {
            let byte = *self.data.get(self.at)?;
            self.at += 1;
            self.cached = self.cached << u8::BITS | u32::from(byte);
            self.held += u8::BITS;
        }
        self.held -= width;
        Some(((self.cached >> self.held) & ((1 << width) - 1)) as u16)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::build;
    use sha2::{Digest as _, Sha256};

    /// A payload that crosses the width the table grows at and repeats bytes, so a round trip exercises
    /// the table's growth and not only its literals.
    fn payload() -> Vec<u8> {
        let mut payload: Vec<u8> = (0..=255_u8).collect();
        payload.extend(std::iter::repeat_n(b'a', 300));
        payload.extend_from_slice(b"the bytes a container states");
        payload
    }

    #[test]
    fn a_stream_decodes_to_the_bytes_it_encodes() {
        let payload = payload();
        let data = build::lzw(&payload);
        assert_eq!(
            decode(&data, 1.0, u64::MAX).as_deref(),
            Some(payload.as_slice())
        );
    }

    #[test]
    fn a_stream_whose_last_code_crosses_the_width_ends_where_it_states() {
        // A table one entry below a power of two when the last code is read: the end marker is read at
        // the width that code's entry moves the table to, one bit past the code's own, so an encoder
        // that states it a bit narrow is read as data and the block padded instead of ended.
        let payload: Vec<u8> = (0..=253).collect();
        let data = build::lzw(&payload);
        assert_eq!(
            decode(&data, 1.0, u64::MAX).as_deref(),
            Some(payload.as_slice())
        );
    }

    #[test]
    fn a_stream_decoding_to_exactly_the_ceiling_is_decoded_and_one_byte_more_is_not() {
        // The ceiling is compared before a code writes, so a container that decodes to exactly it is
        // decoded and one that would pass it is not: the boundary is the ceiling's own value, pinned
        // here through the injected bound rather than at the four gibibytes the engine ships.
        let payload = payload();
        let data = build::lzw(&payload);
        let ceiling = u64::try_from(payload.len()).expect("a payload this host holds");
        assert_eq!(
            decode(&data, 1.0, ceiling).as_deref(),
            Some(payload.as_slice())
        );
        assert!(decode(&data, 1.0, ceiling - 1).is_none());
    }

    #[test]
    fn a_stream_past_the_ceiling_is_stopped_before_its_content_exists() {
        // A code past the table is the one-step repeat, so each one adds a byte to the sequence the
        // next emits: two hundred and forty of them state twenty-nine thousand bytes from two hundred
        // and seventy. The buffer is the caller's, so its capacity is the evidence: a decode that built
        // its output and measured it afterwards would hold all of it before the ceiling was consulted.
        let mut codes = vec![(u16::from(b'A'), 9)];
        codes.extend(std::iter::repeat_n((511_u16, 9), 240));
        let data = build::lzw_codes(&codes);
        let stated = 241 * 242 / 2;
        let ceiling = 4096_u64;
        assert!(
            stated > usize::try_from(ceiling).expect("a small ceiling") * 4,
            "the stream states more than four times the ceiling"
        );
        let mut out = Vec::new();
        assert!(!decode_into(&data, 1.0, ceiling, &mut out));
        assert!(
            out.len() as u64 <= ceiling,
            "{} bytes were produced under a ceiling of {ceiling}",
            out.len()
        );
        assert!(
            out.capacity() as u64 <= ceiling,
            "the buffer reserved {} bytes under a ceiling of {ceiling}",
            out.capacity()
        );
    }

    #[test]
    fn a_stream_that_ends_inside_a_block_is_padded_with_zero_codes() {
        // The reference's decoder emits one zero code for every code its block had left rather than
        // reporting the cut, so a container without an end-of-data code is decoded, not carried.
        let data = build::lzw_codes(&[
            (u16::from(b'a'), 9),
            (u16::from(b'b'), 9),
            (u16::from(b'c'), 9),
            (u16::from(b'd'), 9),
            (u16::from(b'e'), 9),
        ]);
        let decoded = decode(&data, 1.0, u64::MAX).expect("a cut stream is decoded");
        assert_eq!(decoded.len(), CODES);
        assert_eq!(&decoded[..5], b"abcde");
        assert!(decoded[5..].iter().all(|&byte| byte == 0));
    }

    #[test]
    fn an_empty_stream_decodes_to_one_block_of_zero_codes() {
        assert_eq!(decode(&[], 1.0, u64::MAX), Some(vec![0; CODES]));
    }

    #[test]
    fn a_code_past_the_table_is_the_one_step_repeat() {
        // Code 258 is the table's first entry and states nothing yet, so the reference appends the
        // previous sequence's first byte rather than refusing the code.
        let data = build::lzw_codes(&[(u16::from(b'A'), 9), (FIRST, 9), (EOD, 9)]);
        assert_eq!(
            decode(&data, 1.0, u64::MAX).as_deref(),
            Some(b"AAA".as_slice())
        );
    }

    #[test]
    fn a_clear_code_states_the_table_from_its_first_entry() {
        let data = build::lzw_codes(&[
            (u16::from(b'A'), 9),
            (CLEAR, 9),
            (u16::from(b'B'), 9),
            (EOD, 9),
        ]);
        assert_eq!(
            decode(&data, 1.0, u64::MAX).as_deref(),
            Some(b"AB".as_slice())
        );
    }

    #[test]
    fn the_early_change_entry_moves_the_width_by_one_code() {
        // `/EarlyChange` is a number the reference uses in its arithmetic rather than a flag, and the
        // format's default is one: the same bytes read with the standard switch are not the same bytes.
        let payload = payload();
        let data = build::lzw(&payload);
        assert_eq!(
            decode(&data, 1.0, u64::MAX).as_deref(),
            Some(payload.as_slice())
        );
        assert_ne!(
            decode(&data, 0.0, u64::MAX).as_deref(),
            Some(payload.as_slice())
        );
    }

    #[test]
    fn an_entry_that_is_not_zero_or_one_moves_the_crossing_point() {
        // The reference recomputes the width only where the table's size plus `/EarlyChange` is a
        // power of two, so a value past the format's default moves where the width grows rather than
        // adding to it: two reaches the same width one code later, and three hundred only at 1024.
        assert_eq!(grown(511, 1.0, MIN_BITS), 10);
        assert_eq!(grown(511, 2.0, MIN_BITS), MIN_BITS);
        assert_eq!(grown(510, 2.0, MIN_BITS), 10);
        assert_eq!(grown(1000, 300.0, MIN_BITS), MIN_BITS);
        assert_eq!(grown(724, 300.0, MIN_BITS), 11);
        // An entry past what an integer cast holds is wrapped modulo two to the thirty-second, as the
        // reference's own coercion wraps it: a multiple of that wrap states no crossing at all.
        assert_eq!(grown(258, 2f64.powi(32), MIN_BITS), MIN_BITS);
        assert_eq!(grown(258, 2f64.powi(63), MIN_BITS), MAX_BITS);
        // Both operands of the test are coerced from a sum computed in doubles, and past two to the
        // fifty-third the subtraction of one rounds back onto the sum: the entry 511 against 512 would
        // be a crossing, but the rounding makes both operands 512, so the crossing is lost and the
        // width stays where it was.
        assert_eq!(grown(512, 2f64.powi(53), MIN_BITS), MIN_BITS);
    }

    #[test]
    fn a_width_below_zero_states_a_literal_byte_per_code_until_the_ceiling() {
        // A parameter whose magnitude is larger than the ceiling never lets the table's entry count
        // pass it: the first two codes, both read at nine bits, leave the width at zero from the
        // third — a code of no bits that states the literal zero and consumes none — so only the
        // ceiling ends the decode.
        let data = build::lzw(&payload());
        let mut out = Vec::new();
        assert!(!decode_into(&data, -1e300, 4096, &mut out));
        assert_eq!(out.len(), 4096);
    }

    #[test]
    fn a_length_carried_across_a_clear_code_past_the_table_states_zeros() {
        // A clear code states the table from its first entry again but keeps the lengths the previous
        // table grew, and a code past the table names one of those: the reference writes into its
        // fixed array, drops the writes past it and states zeros where they would have been, so the
        // sequence is longer than the table rather than an index past it.
        //
        // The codes `FIRST`..=`end` of a table grown from its first entry, each at the width the
        // decoder reads it, and the width the code after them is read at. `lag` is one where the run's
        // first code stores no entry, which moves the width's crossing a code.
        fn run(end: u16, lag: usize) -> (Vec<(u16, u32)>, u32) {
            let mut codes = Vec::new();
            let mut width = MIN_BITS;
            let mut next = usize::from(FIRST) - lag;
            for code in FIRST..=end {
                codes.push((code, width));
                next += 1;
                width = grown(next, 1.0, width);
            }
            (codes, width)
        }
        let last = u16::try_from(TABLE - 1).expect("a code below the table");
        // The entry the first table ends with states one byte per entry it took to grow it.
        let longest = TABLE - 1 - usize::from(LITERALS);
        // The table a clear code restates has to reach twelve bits again before it can name that
        // entry, and the two codes that name it carry its length into the restated table, where an
        // entry a byte apart grows past the table's own size.
        let wide = u16::try_from(2_usize.pow(MAX_BITS - 1) - 1).expect("a code below the table");
        let over = u16::try_from(usize::from(wide) + 1 + 2 * (TABLE - longest))
            .expect("a code below the table");
        let mut codes = vec![(u16::from(b'A'), MIN_BITS)];
        let (mut first, width) = run(last, 0);
        codes.append(&mut first);
        codes.push((CLEAR, width));
        let (mut table, width) = run(wide, 1);
        codes.append(&mut table);
        // A code past the table is the one-step repeat, and it states the entry the code before it
        // names: the first names the restated table's own last entry, the second carries the length
        // that entry still holds into the table the clear code restated.
        codes.push((last, width));
        codes.push((last, MAX_BITS));
        codes.extend((usize::from(wide) + 1..=usize::from(over)).map(|code| {
            (
                u16::try_from(code).expect("a code below the table"),
                MAX_BITS,
            )
        }));
        codes.push((EOD, MAX_BITS));
        let decoded = decode(&build::lzw_codes(&codes), 1.0, u64::MAX)
            .expect("a stream with two tables is decoded");
        assert_eq!(
            decoded.last(),
            Some(&0),
            "the byte past the table is a zero"
        );
        // The bytes the reference's own decoder states for this stream, pinned so a walk that lands
        // the chain's writes one entry away is not mistaken for the same answer.
        let mut hasher = Sha256::new();
        hasher.update(&decoded);
        assert_eq!(
            format!("{:x}", hasher.finalize()),
            "750d38fd43eb28b073e9d102fceea1ba5f7d07387ef7ba7b1f52ac51e41f6a85"
        );
    }
}
