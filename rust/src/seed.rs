//! Seed codes for the frontend: format `XXX-XXX`, alphabet `[0-9A-Z]`.
//!
//! A regular user doesn't need a u64: a 6-character code (36^6 ≈ 2.2 billion
//! variants) can be typed, shown and shared. The backend stores the u64;
//! the code is its canonical representation (the low 36^6 values).
//! Garbage input is sanitized: case is normalized, stray characters
//! are discarded; fewer than 6 significant characters is rejected (the frontend keeps the old value).

/// Number of distinct codes: 36^6.
pub const CODE_SPACE: u64 = 36 * 36 * 36 * 36 * 36 * 36;

const ALPHABET: &[u8; 36] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";

fn digit_value(byte: u8) -> Option<u64> {
    let upper = byte.to_ascii_uppercase();
    ALPHABET.iter().position(|&d| d == upper).map(|i| i as u64)
}

/// Canonical seed code: uppercase `XXX-XXX`.
pub fn format_seed_code(seed: u64) -> String {
    let mut value = seed % CODE_SPACE;
    let mut digits = [0u8; 6];
    for i in (0..6).rev() {
        digits[i] = ALPHABET[(value % 36) as usize];
        value /= 36;
    }
    let left: String = digits[..3].iter().map(|&b| b as char).collect();
    let right: String = digits[3..].iter().map(|&b| b as char).collect();
    format!("{left}-{right}")
}

/// Code parsing: dashes/spaces/garbage are ignored, case makes no difference.
/// Exactly 6 significant characters are required, otherwise `None` (nothing changes).
pub fn parse_seed_code(code: &str) -> Option<u64> {
    let mut value: u64 = 0;
    let mut count = 0;
    for byte in code.bytes() {
        if byte == b'-' || !byte.is_ascii_alphanumeric() {
            continue;
        }
        let digit = digit_value(byte)?;
        if count >= 6 {
            break;
        }
        value = value * 36 + digit;
        count += 1;
    }
    if count == 6 {
        Some(value)
    } else {
        None
    }
}

/// Random code for the frontend's "dice" button.
pub fn random_seed_code() -> String {
    use rand::Rng;
    let seed: u64 = rand::thread_rng().gen();
    format_seed_code(seed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        for seed in [0, 1, 42, 424242, 123456789, u64::MAX] {
            let code = format_seed_code(seed);
            assert_eq!(code.len(), 7, "code {code}");
            assert_eq!(code.as_bytes()[3], b'-');
            assert!(code.bytes().all(|b| b == b'-' || digit_value(b).is_some()));
            assert_eq!(parse_seed_code(&code), Some(seed % CODE_SPACE));
        }
    }

    #[test]
    fn sanitizes_input() {
        assert_eq!(parse_seed_code("ab12c"), None); // short: 5 characters
        assert!(parse_seed_code("ab12-cd").is_some()); // the dash doesn't count
        assert_eq!(parse_seed_code("ab12cd34"), parse_seed_code("AB12CD"));
        assert_eq!(parse_seed_code("  aB-12-cD\n"), parse_seed_code("AB12CD"));
        assert_eq!(parse_seed_code("AB!@#12CD"), parse_seed_code("AB12CD"));
        assert_eq!(parse_seed_code("------"), None);
        assert_eq!(parse_seed_code(""), None);
    }

    #[test]
    fn random_code_is_valid() {
        for _ in 0..10 {
            let code = random_seed_code();
            assert!(parse_seed_code(&code).is_some(), "bad code {code}");
        }
    }
}
