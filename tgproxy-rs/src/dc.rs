//! Константы датацентров Telegram и выбор WS-доменов (порт utils.py).

/// Основные адреса прямых WS-подключений к DC. Для DC2/DC4 — дефолт
/// `dc_redirects` апстрима (149.154.167.220), для остальных — таблица
/// DC_DEFAULT_IPS (у апстрима она используется только как TCP-фолбэк,
/// но для нестандартных DC других адресов нет).
pub fn dc_ws_ip(dc: u16) -> Option<&'static str> {
    Some(match dc {
        2 | 4 => "149.154.167.220",
        1 => "149.154.175.50",
        3 => "149.154.175.100",
        5 => "149.154.171.5",
        203 => "91.105.192.100",
        _ => return None,
    })
}

/// IP тестовых датацентров (порт DC_TEST_IPS).
pub fn dc_test_ip(dc: u16) -> Option<&'static str> {
    Some(match dc {
        1 => "149.154.175.10",
        2 => "149.154.167.40",
        3 => "149.154.175.117",
        _ => return None,
    })
}

/// Bootstrap-адреса raw MTProto TCP. Позже этот список будет дополняться
/// last-known-good данными из getProxyConfig/getProxyConfigV6, но статический
/// набор нужен, чтобы updater мог стартовать даже в полностью заблокированной
/// сети. Порядок соответствует публичной конфигурации Telegram/MTProxy.
pub fn dc_tcp_endpoints(dc: u16, is_test: bool) -> &'static [(&'static str, u16)] {
    if is_test {
        return match dc {
            1 => &[("149.154.175.10", 443)],
            2 => &[("149.154.167.40", 443)],
            3 => &[("149.154.175.117", 443)],
            _ => &[],
        };
    }

    match dc {
        1 => &[("149.154.175.50", 443)],
        2 => &[("149.154.167.51", 443), ("95.161.76.100", 443)],
        3 => &[("149.154.175.100", 443)],
        4 => &[("149.154.167.91", 443)],
        5 => &[("149.154.171.5", 443)],
        203 => &[("91.105.192.100", 443)],
        _ => &[],
    }
}

pub const WS_PATH: &str = "/apiws";
pub const WS_PATH_TEST: &str = "/apiws_test";

/// Порядок WS-доменов для прямого подключения (порт ws_domains):
/// media-клиенты получают kws{dc}-1 первым.
pub fn ws_domains(dc: u16, is_media: bool) -> Vec<String> {
    let dc = if dc == 203 { 2 } else { dc };
    let primary = format!("kws{dc}.web.telegram.org");
    let secondary = format!("kws{dc}-1.web.telegram.org");
    if is_media {
        vec![secondary, primary]
    } else {
        vec![primary, secondary]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ws_ips_cover_production_dcs() {
        assert_eq!(dc_ws_ip(2), Some("149.154.167.220"));
        assert_eq!(dc_ws_ip(4), Some("149.154.167.220"));
        assert_eq!(dc_ws_ip(1), Some("149.154.175.50"));
        assert_eq!(dc_ws_ip(203), Some("91.105.192.100"));
        assert_eq!(dc_ws_ip(6), None);
    }

    #[test]
    fn media_reorders_domains() {
        assert_eq!(
            ws_domains(2, false),
            vec![
                "kws2.web.telegram.org".to_string(),
                "kws2-1.web.telegram.org".to_string()
            ]
        );
        assert_eq!(
            ws_domains(2, true),
            vec![
                "kws2-1.web.telegram.org".to_string(),
                "kws2.web.telegram.org".to_string()
            ]
        );
    }

    #[test]
    fn dc203_maps_to_kws2() {
        assert_eq!(ws_domains(203, false)[0], "kws2.web.telegram.org");
    }

    #[test]
    fn tcp_bootstrap_covers_regular_cdn_and_test_dcs() {
        assert_eq!(dc_tcp_endpoints(1, false), &[("149.154.175.50", 443)]);
        assert_eq!(dc_tcp_endpoints(203, false), &[("91.105.192.100", 443)]);
        assert_eq!(dc_tcp_endpoints(2, true), &[("149.154.167.40", 443)]);
        assert!(dc_tcp_endpoints(999, false).is_empty());
    }
}
