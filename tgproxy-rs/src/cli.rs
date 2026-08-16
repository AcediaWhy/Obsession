//! CLI-контракт, совместимый с запуском из Obsession (`proxy.rs::start_locked`)
//! и поведением argparse апстрима: неизвестный флаг — ошибка, код выхода 2.

use std::path::PathBuf;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub const USAGE: &str = "\
tg_ws_proxy — headless MTProto<->WebSocket proxy for Obsession

Usage:
  tg_ws_proxy --port <port> --secret <secret> [options]

Options:
  --port <port>                 TCP port to listen on (required)
  --secret <secret>             16-byte secret as 32 hex chars (required)
  --fake-tls-domain <domain>    enable FakeTLS masking for the given domain
  --cfproxy                     enable public CF relay fallback (off by default)
  --cfproxy-cache <path>        cache of last-good public CF relay domains
  --cfproxy-worker-domain <d>   own Cloudflare Worker relay domain (repeatable / comma-separated)
  --no-cfproxy                  explicitly disable public CF relay fallback
  --no-direct                   debug: skip direct DC connections, force fallback
  --host <addr>                 listen address (default: 127.0.0.1)
  -h, --help                    print this help
  --version                     print version
";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub host: String,
    pub port: u16,
    /// 32 hex-символа (16 байт), нормализованы к нижнему регистру.
    pub secret: String,
    pub fake_tls_domain: Option<String>,
    /// Obsession всегда передаёт этот флаг; кэш хранит порядок последних
    /// рабочих CF relay доменов.
    pub cfproxy_cache: Option<PathBuf>,
    /// Домены собственных Cloudflare Worker'ов: приоритетный фолбэк,
    /// зависящий только от владельца.
    pub cfproxy_worker_domains: Vec<String>,
    /// Отключает публичный CF relay. По умолчанию `true`: сторонние relay
    /// включаются только явным `--cfproxy`.
    pub no_cfproxy: bool,
    /// Отладка: мимо прямого пути, сразу в фолбэк (worker/relay).
    pub no_direct: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Parsed {
    Run(Args),
    Help(String),
    Version(String),
}

pub fn parse(argv: &[String]) -> Result<Parsed, String> {
    let mut host: Option<String> = None;
    let mut port: Option<u16> = None;
    let mut secret: Option<String> = None;
    let mut fake_tls_domain: Option<String> = None;
    let mut cfproxy_cache: Option<PathBuf> = None;
    let mut cfproxy_worker_domains: Vec<String> = Vec::new();
    let mut no_cfproxy = true;
    let mut no_direct = false;

    let mut index = 0usize;
    while let Some(flag) = argv.get(index).map(String::as_str) {
        let mut value = |flag: &str| -> Result<String, String> {
            index += 1;
            argv.get(index)
                .cloned()
                .ok_or_else(|| format!("`{flag}` expects a value"))
        };
        match flag {
            "-h" | "--help" => return Ok(Parsed::Help(USAGE.to_string())),
            "--version" => return Ok(Parsed::Version(format!("tg_ws_proxy {VERSION}"))),
            "--host" => host = Some(value(flag)?),
            "--port" => port = Some(parse_port(&value(flag)?)?),
            "--secret" => secret = Some(parse_secret(&value(flag)?)?),
            "--fake-tls-domain" => fake_tls_domain = Some(parse_fake_tls_domain(&value(flag)?)?),
            "--cfproxy-cache" => cfproxy_cache = Some(PathBuf::from(value(flag)?)),
            "--cfproxy-worker-domain" => {
                for part in value(flag)?.split(',') {
                    cfproxy_worker_domains.push(parse_domain(part)?);
                }
            }
            "--cfproxy" => no_cfproxy = false,
            "--no-cfproxy" => no_cfproxy = true,
            "--no-direct" => no_direct = true,
            other => return Err(format!("unknown flag `{other}`")),
        }
        index += 1;
    }

    let port = port.ok_or("`--port` is required")?;
    let secret = secret.ok_or("`--secret` is required")?;
    Ok(Parsed::Run(Args {
        host: host.unwrap_or_else(|| "127.0.0.1".to_string()),
        port,
        secret,
        fake_tls_domain,
        cfproxy_cache,
        cfproxy_worker_domains,
        no_cfproxy,
        no_direct,
    }))
}

fn parse_port(raw: &str) -> Result<u16, String> {
    let port: u16 = raw
        .parse()
        .map_err(|_| format!("invalid port `{raw}` (expected 1..=65535)"))?;
    if port == 0 {
        return Err(format!("invalid port `{raw}` (expected 1..=65535)"));
    }
    Ok(port)
}

fn parse_secret(raw: &str) -> Result<String, String> {
    if raw.len() != 32 || !raw.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("secret must be 32 hex chars (16 bytes)".to_string());
    }
    Ok(raw.to_ascii_lowercase())
}

fn parse_fake_tls_domain(raw: &str) -> Result<String, String> {
    let domain = parse_domain(raw)?;
    if domain.is_empty() {
        return Err(format!("invalid fake TLS domain `{raw}`"));
    }
    Ok(domain)
}

/// Строгая ASCII-валидация DNS-имени: метки 1..=63, без `_` и дефиса
/// по краям. IP-адреса и URL здесь не принимаются.
fn parse_domain(raw: &str) -> Result<String, String> {
    let domain = raw.trim();
    let valid = !domain.is_empty()
        && domain.len() <= 253
        && domain.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
                && !label.starts_with('-')
                && !label.ends_with('-')
        });
    if valid {
        Ok(domain.to_string())
    } else {
        Err(format!("invalid domain `{domain}`"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    fn run_args(items: &[&str]) -> Args {
        match parse(&argv(items)).expect("parse must succeed") {
            Parsed::Run(args) => args,
            other => panic!("expected Run, got {other:?}"),
        }
    }

    #[test]
    fn parses_full_obsession_contract() {
        let args = run_args(&[
            "--port",
            "1443",
            "--secret",
            "00112233445566778899aabbccddeeff",
            "--fake-tls-domain",
            "www.google.com",
            "--cfproxy-cache",
            "C:\\cfproxy\\cache.json",
        ]);
        assert_eq!(args.host, "127.0.0.1");
        assert_eq!(args.port, 1443);
        assert_eq!(args.secret, "00112233445566778899aabbccddeeff");
        assert_eq!(args.fake_tls_domain.as_deref(), Some("www.google.com"));
        assert_eq!(
            args.cfproxy_cache,
            Some(PathBuf::from("C:\\cfproxy\\cache.json"))
        );
        assert!(args.no_cfproxy, "public relays must be opt-in");
    }

    #[test]
    fn uppercase_secret_is_normalized() {
        let args = run_args(&[
            "--port",
            "1",
            "--secret",
            "AABB00112233445566778899AABBCCDD",
        ]);
        assert_eq!(args.secret, "aabb00112233445566778899aabbccdd");
    }

    #[test]
    fn custom_host_is_kept() {
        let args = run_args(&[
            "--host",
            "192.168.1.5",
            "--port",
            "1443",
            "--secret",
            "00112233445566778899aabbccddeeff",
        ]);
        assert_eq!(args.host, "192.168.1.5");
    }

    #[test]
    fn rejects_unknown_flag() {
        let error = parse(&argv(&[
            "--port",
            "1",
            "--secret",
            "00112233445566778899aabbccddeeff",
            "--bogus",
        ]))
        .unwrap_err();
        assert!(error.contains("--bogus"), "error was: {error}");
    }

    #[test]
    fn rejects_missing_value() {
        let error = parse(&argv(&["--port"])).unwrap_err();
        assert!(error.contains("--port"), "error was: {error}");
    }

    #[test]
    fn rejects_short_and_non_hex_secret() {
        assert!(parse(&argv(&["--port", "1", "--secret", "aabb"])).is_err());
        assert!(parse(&argv(&["--port", "1", "--secret", &"z".repeat(32)])).is_err());
    }

    #[test]
    fn rejects_zero_and_overflow_port() {
        assert!(parse(&argv(&[
            "--port",
            "0",
            "--secret",
            "00112233445566778899aabbccddeeff"
        ]))
        .is_err());
        assert!(parse(&argv(&[
            "--port",
            "65536",
            "--secret",
            "00112233445566778899aabbccddeeff"
        ]))
        .is_err());
    }

    #[test]
    fn requires_port_and_secret() {
        let error = parse(&argv(&[])).unwrap_err();
        assert!(error.contains("--port"), "error was: {error}");
        let error = parse(&argv(&["--port", "1443"])).unwrap_err();
        assert!(error.contains("--secret"), "error was: {error}");
    }

    #[test]
    fn rejects_empty_fake_tls_domain() {
        assert!(parse(&argv(&[
            "--port",
            "1",
            "--secret",
            "00112233445566778899aabbccddeeff",
            "--fake-tls-domain",
            "   "
        ]))
        .is_err());
    }

    #[test]
    fn help_and_version_short_circuit() {
        assert!(matches!(parse(&argv(&["--help"])), Ok(Parsed::Help(_))));
        assert!(matches!(parse(&argv(&["-h"])), Ok(Parsed::Help(_))));
        assert!(matches!(
            parse(&argv(&["--version"])),
            Ok(Parsed::Version(_))
        ));
    }

    #[test]
    fn no_cfproxy_flag_defaults_false_and_parses() {
        let args = run_args(&[
            "--port",
            "1",
            "--secret",
            "00112233445566778899aabbccddeeff",
        ]);
        assert!(args.no_cfproxy);
        assert!(args.cfproxy_worker_domains.is_empty());

        let args = run_args(&[
            "--port",
            "1",
            "--secret",
            "00112233445566778899aabbccddeeff",
            "--cfproxy",
        ]);
        assert!(!args.no_cfproxy);

        let args = run_args(&[
            "--port",
            "1",
            "--secret",
            "00112233445566778899aabbccddeeff",
            "--cfproxy",
            "--no-cfproxy",
        ]);
        assert!(args.no_cfproxy);
    }

    #[test]
    fn no_direct_flag_parses() {
        let args = run_args(&[
            "--port",
            "1",
            "--secret",
            "00112233445566778899aabbccddeeff",
            "--no-direct",
        ]);
        assert!(args.no_direct);

        let args = run_args(&[
            "--port",
            "1",
            "--secret",
            "00112233445566778899aabbccddeeff",
        ]);
        assert!(!args.no_direct);
    }

    #[test]
    fn worker_domains_parse_comma_separated_and_repeated() {
        let args = run_args(&[
            "--port",
            "1",
            "--secret",
            "00112233445566778899aabbccddeeff",
            "--cfproxy-worker-domain",
            "one.workers.dev, two.workers.dev",
            "--cfproxy-worker-domain",
            "three.workers.dev",
        ]);
        assert_eq!(
            args.cfproxy_worker_domains,
            vec![
                "one.workers.dev".to_string(),
                "two.workers.dev".to_string(),
                "three.workers.dev".to_string()
            ]
        );

        // Мусорные значения отвергаются.
        assert!(parse(&argv(&[
            "--port",
            "1",
            "--secret",
            "00112233445566778899aabbccddeeff",
            "--cfproxy-worker-domain",
            "https://evil.example/path"
        ]))
        .is_err());

        for invalid in ["bad_name.example", "-bad.example", "bad-.example", "a..b"] {
            assert!(parse(&argv(&[
                "--port",
                "1",
                "--secret",
                "00112233445566778899aabbccddeeff",
                "--cfproxy-worker-domain",
                invalid
            ]))
            .is_err());
        }
    }
}
