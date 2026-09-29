//! Сид-коды для фронта: формат `XXX-XXX`, алфавит `[0-9A-Z]`.
//!
//! Обычному пользователю u64 не нужен: код из 6 символов (36^6 ≈ 2.2 млрд
//! вариантов) вводится, показывается и шарится. Бакенд хранит u64;
//! код — его каноническое отображение (младшие 36^6).
//! Ввод мусора санитизируется: регистр выравнивается, левые символы
//! выбрасываются; короче 6 значимых — отклоняется (фронт держит старый).

/// Число разных кодов: 36^6.
pub const CODE_SPACE: u64 = 36 * 36 * 36 * 36 * 36 * 36;

const ALPHABET: &[u8; 36] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";

fn digit_value(byte: u8) -> Option<u64> {
    let upper = byte.to_ascii_uppercase();
    ALPHABET.iter().position(|&d| d == upper).map(|i| i as u64)
}

/// Канонический код сида: `XXX-XXX` в верхнем регистре.
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

/// Разбор кода: дефисы/пробелы/мусор игнорируются, регистр — нет разницы.
/// Нужно ровно 6 значимых символов, иначе `None` (ничего не меняем).
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

/// Случайный код для кнопки «кубик» во фронте.
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
        assert_eq!(parse_seed_code("ab12c"), None); // коротко: 5 символов
        assert!(parse_seed_code("ab12-cd").is_some()); // дефис не в счет
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
