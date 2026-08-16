//! Сборка tg://proxy-ссылки. Формат байт-в-байт как в апстриме
//! (proxy/tg_ws_proxy.py): `server`, `port`, `secret=dd<secret>` либо
//! `ee<secret><hex(domain)>` при включённом FakeTLS.

/// Hex-кодирование ASCII-домена для ee-секрета — нижний регистр,
/// эквивалент `.encode('ascii').hex()` в Python.
fn hex_ascii(domain: &str) -> String {
    domain.bytes().map(|byte| format!("{byte:02x}")).collect()
}

pub fn build_link(
    host: &str,
    port: u16,
    secret_hex: &str,
    fake_tls_domain: Option<&str>,
) -> String {
    let secret = match fake_tls_domain {
        Some(domain) => format!("ee{secret_hex}{}", hex_ascii(domain)),
        None => format!("dd{secret_hex}"),
    };
    format!("tg://proxy?server={host}&port={port}&secret={secret}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "00112233445566778899aabbccddeeff";

    #[test]
    fn dd_link_matches_upstream_format() {
        assert_eq!(
            build_link("127.0.0.1", 1443, SECRET, None),
            "tg://proxy?server=127.0.0.1&port=1443&secret=dd00112233445566778899aabbccddeeff"
        );
    }

    #[test]
    fn ee_link_appends_lowercase_domain_hex() {
        // www.google.com -> 77 77 77 2e 67 6f 6f 67 6c 65 2e 63 6f 6d
        assert_eq!(
            build_link("127.0.0.1", 1443, SECRET, Some("www.google.com")),
            concat!(
                "tg://proxy?server=127.0.0.1&port=1443",
                "&secret=ee00112233445566778899aabbccddeeff7777772e676f6f676c652e636f6d"
            )
        );
    }

    #[test]
    fn link_has_no_whitespace_for_obsession_regex() {
        let link = build_link("127.0.0.1", 1443, SECRET, Some("www.cloudflare.com"));
        assert!(link.starts_with("tg://proxy?"));
        assert!(!link.chars().any(char::is_whitespace));
    }
}
