//! Выходной сигнал «глаз»: сырой per-flow вердикт по одному соединению.
//! Глаза НЕ агрегируют — каждое соединение даёт одно наблюдение, всю
//! агрегацию/гистерезис делает «мозг» (машина состояний) выше по стеку.

use std::net::IpAddr;

use serde::Serialize;

/// Вердикт по одному потоку.
/// - `Working`   — пришёл ServerHello/данные, обход сработал.
/// - `Reset`     — входящий RST: DPI срезал именно эту попытку (можно пробовать другую стратегию).
/// - `Blackhole` — тишина + ретрансмиты: дроп по направлению (замереть, не перебирать).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    Working,
    Reset,
    Blackhole,
}

/// Нормализованный диагноз типа сетевого вмешательства (WS4 §9.2). Богаче, чем
/// `Verdict`: различает транспорт (TCP/UDP/QUIC/DNS) и характер блокировки. Пока
/// АДДИТИВНЫЙ слой — Мозг продолжает работать на `Verdict` (проверенный TCP-путь),
/// а UDP/QUIC/DNS-наблюдатели и WS5 будут формировать/потреблять `Diagnosis`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Diagnosis {
    /// Соединение живо (ServerHello/данные/ответ).
    Working,
    /// DNS-резолв не удался (NXDOMAIN/таймаут/подмена).
    DnsFailure,
    /// Входящий TCP RST — DPI срезал попытку.
    TcpReset,
    /// TCP: SYN без SYN-ACK, дроп на уровне рукопожатия.
    TcpBlackhole,
    /// TLS: ClientHello ушёл, ответа нет (дроп по TLS-содержимому).
    TlsBlackhole,
    /// QUIC Initial блокируется (UDP/443 без ответа на Initial).
    QuicBlocked,
    /// UDP-поток блокируется (не-QUIC, напр. STUN/Discord voice).
    UdpBlocked,
    /// HTTP-страница-заглушка провайдера вместо контента.
    HttpBlockPage,
    /// Сильное замедление (throttling) без полного дропа.
    Throttled,
    /// IP недостижим (маршрут/ICMP unreachable), не DPI.
    IpUnreachable,
    /// Не удалось классифицировать.
    Unknown,
}

impl Diagnosis {
    /// Мост из проверенного TCP-`Verdict` в нормализованный диагноз. Blackhole по
    /// потоку с известным доменом трактуем как TLS-blackhole (вердикт выносится
    /// после ClientHello); SYN-level blackhole различает вызывающий по evidence.
    pub fn from_verdict(v: Verdict) -> Diagnosis {
        match v {
            Verdict::Working => Diagnosis::Working,
            Verdict::Reset => Diagnosis::TcpReset,
            Verdict::Blackhole => Diagnosis::TlsBlackhole,
        }
    }

    /// Более точный мост с учётом evidence (различает SYN- и TLS-level blackhole).
    pub fn from_verdict_evidence(v: Verdict, evidence: &str) -> Diagnosis {
        match (v, evidence) {
            (Verdict::Blackhole, "syn_no_synack") => Diagnosis::TcpBlackhole,
            _ => Diagnosis::from_verdict(v),
        }
    }

    /// Стабильный строковый тег (для логов/фронта/кэша).
    pub fn tag(self) -> &'static str {
        match self {
            Diagnosis::Working => "working",
            Diagnosis::DnsFailure => "dns_failure",
            Diagnosis::TcpReset => "tcp_reset",
            Diagnosis::TcpBlackhole => "tcp_blackhole",
            Diagnosis::TlsBlackhole => "tls_blackhole",
            Diagnosis::QuicBlocked => "quic_blocked",
            Diagnosis::UdpBlocked => "udp_blocked",
            Diagnosis::HttpBlockPage => "http_block_page",
            Diagnosis::Throttled => "throttled",
            Diagnosis::IpUnreachable => "ip_unreachable",
            Diagnosis::Unknown => "unknown",
        }
    }

    /// Означает ли диагноз рабочее соединение.
    pub fn is_working(self) -> bool {
        matches!(self, Diagnosis::Working)
    }
}

/// Наблюдение по одному потоку — то, что эмитится во фронтенд (событие `eyes://observation`).
/// Плоское и сериализуемое: фронт просто рисует статус per-domain, ничего не вычисляя.
#[derive(Clone, Debug, Serialize)]
pub struct Observation {
    /// SNI-хост, к которому шло соединение (из ClientHello).
    pub domain: String,
    /// Удалённый IP (serde сериализует `IpAddr` как строку).
    pub dst_ip: IpAddr,
    /// Локальный порт — идентификатор конкретного соединения внутри домена.
    pub local_port: u16,
    pub verdict: Verdict,
    /// Признак, по которому вынесен вердикт: "server_hello" | "inbound_rst"
    /// | "silence+retransmit" | "silence" | "syn_no_synack".
    pub evidence: &'static str,
    /// Логическое время события в мс (передаётся снаружи ради детерминизма тестов).
    pub ts_ms: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verdict_bridges_to_diagnosis() {
        assert_eq!(
            Diagnosis::from_verdict(Verdict::Working),
            Diagnosis::Working
        );
        assert_eq!(Diagnosis::from_verdict(Verdict::Reset), Diagnosis::TcpReset);
        assert_eq!(
            Diagnosis::from_verdict(Verdict::Blackhole),
            Diagnosis::TlsBlackhole
        );
    }

    #[test]
    fn evidence_distinguishes_syn_from_tls_blackhole() {
        // SYN-level blackhole → TcpBlackhole.
        assert_eq!(
            Diagnosis::from_verdict_evidence(Verdict::Blackhole, "syn_no_synack"),
            Diagnosis::TcpBlackhole
        );
        // Blackhole после ClientHello → TlsBlackhole.
        assert_eq!(
            Diagnosis::from_verdict_evidence(Verdict::Blackhole, "silence+retransmit"),
            Diagnosis::TlsBlackhole
        );
        // Reset не зависит от evidence.
        assert_eq!(
            Diagnosis::from_verdict_evidence(Verdict::Reset, "inbound_rst"),
            Diagnosis::TcpReset
        );
    }

    #[test]
    fn tags_are_stable_snake_case() {
        assert_eq!(Diagnosis::QuicBlocked.tag(), "quic_blocked");
        assert_eq!(Diagnosis::DnsFailure.tag(), "dns_failure");
        assert!(Diagnosis::Working.is_working());
        assert!(!Diagnosis::TcpReset.is_working());
    }

    #[test]
    fn diagnosis_serializes_snake_case() {
        let json = serde_json::to_string(&Diagnosis::HttpBlockPage).unwrap();
        assert_eq!(json, "\"http_block_page\"");
    }
}
