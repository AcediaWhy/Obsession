//! Скользящее окно per-flow вердиктов Глаз — источник агрегатов для машины
//! состояний Мозга. Чистое, на логическом времени (`ts` приходит снаружи),
//! без tokio/WinDivert — тестируется на фикстурах детерминированно.
//!
//! Мозг НЕ доверяет одиночному наблюдению: один шумный поток не должен гнать ни
//! респавн (`resets`), ни заморозку (`blackholes`). Окно даёт счётчики за
//! последние `window_ms` мс логического времени.

use std::collections::VecDeque;

use crate::eyes::Verdict;

/// Одно наблюдение в окне: время, домен-источник, вердикт.
#[derive(Clone, Debug)]
struct Entry {
    ts: u64,
    domain: String,
    verdict: Verdict,
}

/// Скользящее окно наблюдений фиксированной длительности.
#[derive(Debug)]
pub struct Window {
    window_ms: u64,
    entries: VecDeque<Entry>,
}

impl Window {
    pub fn new(window_ms: u64) -> Self {
        Self {
            window_ms,
            entries: VecDeque::new(),
        }
    }

    /// Добавляет наблюдение и обрезает всё старше `now - window_ms`.
    pub fn push(&mut self, domain: &str, verdict: Verdict, now: u64) {
        self.entries.push_back(Entry {
            ts: now,
            domain: domain.to_string(),
            verdict,
        });
        self.prune(now);
    }

    /// Обрезает устаревшие записи. Вызывается и на push, и на тик (когда новых
    /// пакетов нет, но окно должно «протекать»).
    pub fn prune(&mut self, now: u64) {
        let cutoff = now.saturating_sub(self.window_ms);
        while let Some(front) = self.entries.front() {
            if front.ts < cutoff {
                self.entries.pop_front();
            } else {
                break;
            }
        }
    }

    /// Число входящих RST по домену `cat` (суффиксный матч не нужен — Глаза уже
    /// отдают конкретный SNI; сопоставление домен→категория делает вызывающий).
    /// Здесь считаем по точному домену, переданному как ключ фильтра.
    pub fn resets_for<F: Fn(&str) -> bool>(&self, in_cat: F) -> u32 {
        self.count(Verdict::Reset, &in_cat)
    }

    /// Число `working` (ServerHello) по фильтру категории.
    #[allow(dead_code)] // задел API окна: подтверждение здоровья идёт по hello_count в model
    pub fn working_for<F: Fn(&str) -> bool>(&self, in_cat: F) -> u32 {
        self.count(Verdict::Working, &in_cat)
    }

    /// Число `blackhole` по ВСЕМ доменам — предохранитель ортогонален категориям
    /// (дроп по IP-направлению морозит весь набор).
    pub fn blackholes(&self) -> u32 {
        self.entries
            .iter()
            .filter(|e| e.verdict == Verdict::Blackhole)
            .count() as u32
    }

    fn count<F: Fn(&str) -> bool>(&self, want: Verdict, in_cat: &F) -> u32 {
        self.entries
            .iter()
            .filter(|e| e.verdict == want && in_cat(&e.domain))
            .count() as u32
    }

    /// Полная очистка (на смене стратегии/сессии — старые вердикты нерелевантны).
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn any(_: &str) -> bool {
        true
    }

    #[test]
    fn prunes_old_entries() {
        let mut w = Window::new(1000);
        w.push("a.com", Verdict::Reset, 0);
        w.push("a.com", Verdict::Reset, 500);
        assert_eq!(w.len(), 2);
        // now=1200 → cutoff=200 → запись на ts=0 выпадает.
        w.push("a.com", Verdict::Reset, 1200);
        assert_eq!(w.len(), 2);
        assert_eq!(w.resets_for(any), 2);
    }

    #[test]
    fn prune_on_tick_without_push() {
        let mut w = Window::new(1000);
        w.push("a.com", Verdict::Blackhole, 0);
        assert_eq!(w.blackholes(), 1);
        w.prune(2000); // тик спустя, новых пакетов нет
        assert_eq!(w.blackholes(), 0);
    }

    #[test]
    fn blackholes_count_all_domains() {
        let mut w = Window::new(10_000);
        w.push("youtube.com", Verdict::Blackhole, 0);
        w.push("discord.com", Verdict::Blackhole, 100);
        assert_eq!(w.blackholes(), 2);
    }

    #[test]
    fn category_filter_scopes_resets() {
        let mut w = Window::new(10_000);
        w.push("youtube.com", Verdict::Reset, 0);
        w.push("discord.com", Verdict::Reset, 100);
        w.push("youtube.com", Verdict::Working, 200);
        let is_yt = |d: &str| d.contains("youtube");
        assert_eq!(w.resets_for(is_yt), 1);
        assert_eq!(w.working_for(is_yt), 1);
    }

    #[test]
    fn clear_empties() {
        let mut w = Window::new(1000);
        w.push("a.com", Verdict::Reset, 0);
        w.clear();
        assert_eq!(w.len(), 0);
    }
}
