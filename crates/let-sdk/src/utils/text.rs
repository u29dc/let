#![forbid(unsafe_code)]

pub fn normalize_postcode(input: &str) -> String {
    input
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>()
        .to_uppercase()
}

pub fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }

    if max_chars <= 1 {
        return "…".to_string();
    }

    let mut out = String::new();
    for (index, ch) in text.chars().enumerate() {
        if index + 1 >= max_chars {
            break;
        }
        out.push(ch);
    }
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::{normalize_postcode, truncate};

    #[test]
    fn postcode_normalizes() {
        assert_eq!(normalize_postcode("sw1a 1aa"), "SW1A1AA");
    }

    #[test]
    fn truncates_with_ellipsis() {
        assert_eq!(truncate("abcdef", 4), "abc…");
    }
}
