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
