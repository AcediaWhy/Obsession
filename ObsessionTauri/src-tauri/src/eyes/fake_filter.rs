//! Отсев собственных fake-инъекций winws на исходящем трафике.
//!
//! winws для десинка сам шлёт поддельные исходящие пакеты (fake ClientHello
//! с низким TTL / badseq). Наблюдатель обязан их игнорировать, иначе примет
//! инъекцию за реальную попытку или посчитает fake+реальный за «ретрансмит».
//!
//! Главный дискриминатор — АНОМАЛЬНО НИЗКИЙ TTL. ОС ставит 64/128; fake winws
//! обычно шлёт с маленьким TTL (autottl часто <20, фиксированный часто 1–8),
//! чтобы пакет умер, не дойдя до сервера. Baseline берём из реального SYN
//! этого же соединения — он всегда настоящий (его шлёт стек ОС).
//!
//! ВАЖНО: контрольную сумму на ИСХОДЯЩИХ не используем — из-за NIC checksum
//! offload у легитимного пакета на слое WinDivert сумма может быть «неверной».
//! badseq оставляем лишь как вторичный хинт, если base_seq известен.

use crate::eyes::parse::ParsedPacket;

/// Насколько TTL должен просесть относительно baseline, чтобы счесть пакет фейком.
/// ОС: 64 или 128. Разумный порог — всё, что заметно ниже минимального baseline.
const TTL_DROP_THRESHOLD: u8 = 24;

/// Контекст соединения, нужный для распознавания фейков.
#[derive(Clone, Copy, Debug, Default)]
pub struct FakeContext {
    /// TTL из реального SYN (baseline «настоящего» пути). 0 — ещё неизвестен.
    pub base_ttl: u8,
    /// Начальный seq из SYN (для эвристики badseq). None — неизвестен.
    pub base_seq: Option<u32>,
}

impl FakeContext {
    /// Запоминает baseline из исходящего SYN.
    pub fn note_syn(&mut self, ttl: u8, seq: u32) {
        self.base_ttl = ttl;
        self.base_seq = Some(seq);
    }
}

/// True, если исходящий пакет похож на инъекцию самого winws, а не на трафик ОС.
///
/// Проверяем только outbound: наши фейки исходящие, inbound ими не засоряется.
pub fn is_winws_fake(pkt: &ParsedPacket, ctx: &FakeContext) -> bool {
    if !pkt.outbound {
        return false;
    }

    // 1. Абсолютно низкий TTL — почти наверняка fake (autottl/fixed).
    //    Даже без baseline: живой исходящий пакет с TTL<=16 в норме не встречается.
    if pkt.ttl > 0 && pkt.ttl <= 16 {
        return true;
    }

    // 2. Относительный провал TTL против baseline из SYN.
    if ctx.base_ttl > 0 && pkt.ttl > 0 {
        if ctx.base_ttl.saturating_sub(pkt.ttl) >= TTL_DROP_THRESHOLD {
            return true;
        }
    }

    // 3. Вторичный хинт: badseq — seq уехал далеко за пределы разумного окна
    //    относительно стартового. Осторожный порог, чтобы не задеть легитимные данные.
    if let Some(base) = ctx.base_seq {
        // wrapping-дистанция в обе стороны.
        let fwd = pkt.seq.wrapping_sub(base);
        let bwd = base.wrapping_sub(pkt.seq);
        const BADSEQ_WINDOW: u32 = 1 << 30; // ~1 ГиБ — заведомо вне реального окна
        if fwd > BADSEQ_WINDOW && bwd > BADSEQ_WINDOW {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eyes::parse::{FlowKey, TcpFlags};
    use std::net::{IpAddr, Ipv4Addr};

    fn pkt(outbound: bool, ttl: u8, seq: u32) -> ParsedPacket {
        ParsedPacket {
            outbound,
            key: FlowKey {
                local_port: 50000,
                remote_ip: IpAddr::V4(Ipv4Addr::new(1, 2, 3, 4)),
                remote_port: 443,
            },
            ttl,
            seq,
            flags: TcpFlags::default(),
            payload: Vec::new(),
        }
    }

    #[test]
    fn low_ttl_is_fake() {
        let ctx = FakeContext::default();
        assert!(is_winws_fake(&pkt(true, 6, 1000), &ctx));
        assert!(is_winws_fake(&pkt(true, 1, 1000), &ctx));
    }

    #[test]
    fn normal_ttl_is_real() {
        let mut ctx = FakeContext::default();
        ctx.note_syn(128, 1000);
        assert!(!is_winws_fake(&pkt(true, 128, 1400), &ctx));
        assert!(!is_winws_fake(&pkt(true, 120, 1400), &ctx));
    }

    #[test]
    fn relative_ttl_drop_is_fake() {
        let mut ctx = FakeContext::default();
        ctx.note_syn(64, 1000);
        // 64 -> 32 это провал на 32 (>=24) — фейк.
        assert!(is_winws_fake(&pkt(true, 32, 1400), &ctx));
    }

    #[test]
    fn inbound_never_fake() {
        let ctx = FakeContext::default();
        // Даже с низким TTL: inbound не фильтруем как fake.
        assert!(!is_winws_fake(&pkt(false, 4, 1000), &ctx));
    }

    #[test]
    fn badseq_far_out_of_window_is_fake() {
        let mut ctx = FakeContext::default();
        ctx.note_syn(64, 1000);
        // seq далеко и вперёд, и назад от base — badseq.
        let far = 1000u32.wrapping_add(1 << 31);
        assert!(is_winws_fake(&pkt(true, 64, far), &ctx));
    }
}
