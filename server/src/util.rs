use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::Rng;
use sha2::{Digest, Sha256};

/// Random URL-safe token with `bytes` bytes of entropy.
pub fn random_token(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    rand::rng().fill(&mut buf[..]);
    URL_SAFE_NO_PAD.encode(buf)
}

/// Short human-friendly code (no ambiguous characters) used in play links.
pub fn random_code(len: usize) -> String {
    const ALPHABET: &[u8] = b"abcdefghjkmnpqrstuvwxyz23456789";
    let mut rng = rand::rng();
    (0..len)
        .map(|_| ALPHABET[rng.random_range(0..ALPHABET.len())] as char)
        .collect()
}

pub fn random_pin() -> String {
    format!("{:04}", rand::rng().random_range(0..10_000))
}

pub fn sha256_hex(input: &str) -> String {
    hex::encode(Sha256::digest(input.as_bytes()))
}

pub fn escape_html(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for c in input.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// Trim, collapse control characters and cap the length of user supplied text.
pub fn clean_text(input: &str, max_chars: usize) -> String {
    let cleaned: String = input
        .chars()
        .map(|c| if c.is_control() && c != '\n' { ' ' } else { c })
        .collect();
    cleaned.trim().chars().take(max_chars).collect()
}

/// Single line variant of [`clean_text`] (names, subjects...).
pub fn clean_line(input: &str, max_chars: usize) -> String {
    clean_text(&input.replace(['\n', '\r'], " "), max_chars)
}

pub fn opt_clean_line(input: Option<&str>, max_chars: usize) -> Option<String> {
    input
        .map(|s| clean_line(s, max_chars))
        .filter(|s| !s.is_empty())
}

/// Pragmatic email validation: one `@`, non-empty local part, dotted domain, no spaces.
pub fn is_valid_email(email: &str) -> bool {
    if email.len() > 254 || email.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return false;
    }
    let Some((local, domain)) = email.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && !domain.contains('@')
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !domain.contains("..")
        && !email.contains(['<', '>', '"', ',', ';'])
}

#[derive(Debug, PartialEq, Eq)]
pub struct ParsedRecipient {
    pub email: String,
    pub name: Option<String>,
}

/// Parse a pasted list of participants. Accepts one entry per line or separated by
/// commas/semicolons, each being `email`, `Name <email>` or `"Name" <email>`.
/// Returns the valid entries (deduplicated, case-insensitive) and the invalid chunks.
pub fn parse_recipients(raw: &str) -> (Vec<ParsedRecipient>, Vec<String>) {
    let mut valid: Vec<ParsedRecipient> = Vec::new();
    let mut invalid = Vec::new();
    for chunk in raw.split(['\n', ',', ';']) {
        let chunk = chunk.trim();
        if chunk.is_empty() {
            continue;
        }
        let (name, email) = match (chunk.find('<'), chunk.rfind('>')) {
            (Some(start), Some(end)) if end > start => {
                let name = chunk[..start].trim().trim_matches('"').trim();
                (
                    (!name.is_empty()).then(|| clean_line(name, 80)),
                    chunk[start + 1..end].trim().to_string(),
                )
            }
            _ => (None, chunk.to_string()),
        };
        let email = email.trim_start_matches("mailto:").to_lowercase();
        if !is_valid_email(&email) {
            invalid.push(chunk.to_string());
            continue;
        }
        if valid.iter().any(|r| r.email == email) {
            continue;
        }
        valid.push(ParsedRecipient { email, name });
    }
    (valid, invalid)
}

/// Normalize a longitude to [-180, 180).
pub fn wrap_lng(lng: f64) -> f64 {
    let wrapped = (lng + 180.0).rem_euclid(360.0) - 180.0;
    if wrapped.is_finite() {
        wrapped
    } else {
        0.0
    }
}

pub fn validate_lat_lng(lat: f64, lng: f64) -> Result<(f64, f64), String> {
    if !lat.is_finite() || !lng.is_finite() || !(-90.0..=90.0).contains(&lat) {
        return Err("Invalid location".into());
    }
    Ok((lat, wrap_lng(lng)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_mixed_recipient_lists() {
        let (valid, invalid) = parse_recipients(
            "alice@example.com\nBob Smith <BOB@example.com>, \"Carol\" <carol@ex.org>; not-an-email\nalice@EXAMPLE.com",
        );
        assert_eq!(
            valid,
            vec![
                ParsedRecipient { email: "alice@example.com".into(), name: None },
                ParsedRecipient { email: "bob@example.com".into(), name: Some("Bob Smith".into()) },
                ParsedRecipient { email: "carol@ex.org".into(), name: Some("Carol".into()) },
            ]
        );
        assert_eq!(invalid, vec!["not-an-email".to_string()]);
    }

    #[test]
    fn email_validation() {
        assert!(is_valid_email("a.b+c@d.co"));
        assert!(!is_valid_email("a@b"));
        assert!(!is_valid_email("a b@c.d"));
        assert!(!is_valid_email("@c.d"));
        assert!(!is_valid_email("a@@c.d"));
    }

    #[test]
    fn wraps_longitudes() {
        assert_eq!(wrap_lng(190.0), -170.0);
        assert_eq!(wrap_lng(-190.0), 170.0);
        assert_eq!(wrap_lng(540.0), -180.0);
        assert_eq!(wrap_lng(12.5), 12.5);
    }

    #[test]
    fn escapes_html() {
        assert_eq!(escape_html("<b>\"x\" & 'y'</b>"), "&lt;b&gt;&quot;x&quot; &amp; &#39;y&#39;&lt;/b&gt;");
    }

    #[test]
    fn codes_and_pins() {
        assert_eq!(random_code(8).len(), 8);
        assert_eq!(random_pin().len(), 4);
        assert!(random_token(32).len() >= 42);
    }
}
