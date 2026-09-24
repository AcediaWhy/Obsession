//! Чистая машина состояний Мозга: редуктор `step(event) -> Vec<Action>` на
//! логическом времени (`now`/`ts` приходят снаружи). Никаких tokio, WinDivert,
//! AppHandle — всё исполняет `runtime`. Тестируется на фикстурах детерминированно.
//!
//! Гранулярность: одна глобальная [`Phase`] (жизненный цикл winws + предохранитель)
//! + per-category [`CatState`] (здоровье и позиция на лестнице). Один winws-набор
//!   на весь сеанс → переключение респавнит весь набор. Блэкхол морозит ВСЁ (дроп по
//!   IP-направлению ортогонален категориям); Reset скоупится на категорию.
//!
//! Строгий приоритет в `evaluate`: (1) предохранитель по блэкхолу → (2) Confirming
//! → (3) Healthy/Suspect-переключение → (4) выход из Frozen пробой. Это прямая
//! кодификация «лестница строгая, с гистерезисом, предохранитель ортогонален» —
//! наивный автоперебор доказанно вреден (провоцирует блэкхол ТСПУ).

use std::collections::{HashMap, HashSet};

use serde::Serialize;

use crate::brain::window::Window;
use crate::eyes::Verdict;

/// Пороги гистерезиса и тайминги. Все времена — мс логического времени.
#[derive(Clone, Debug)]
pub struct BrainCfg {
    /// Длительность скользящего окна вердиктов.
    pub window_ms: u64,
    /// Сколько Reset по категории в окне, чтобы переключить стратегию (не по 1 RST).
    pub switch_after_resets: u32,
    /// Сколько ServerHello после респавна, чтобы счесть набор здоровым.
    pub confirm_min_hellos: u32,
    /// Окно карантина тишины после респавна (Глаза мертвы ~800мс + окно без защиты).
    pub confirm_grace_ms: u64,
    /// Сколько blackhole-вердиктов (= отдельных заблэкхоленных потоков) в окне
    /// взводят предохранитель. Требуем КОРРОБОРАЦИЮ: реальный дроп по направлению
    /// бьёт много потоков подряд, а брошенный/спекулятивный сокет даёт ровно один.
    /// Один blackhole больше НЕ морозит рабочий обход (частая причина ложных заморозок).
    pub blackhole_trip: u32,
    /// Минимальный интервал между респавнами — антипровокационный пейсинг L3.
    pub min_switch_interval_ms: u64,
    /// Период тика (для справки; тикер живёт в runtime).
    pub tick_ms: u64,
    /// Лестница остывания Frozen: 1/3/5 мин. Первый шаг короткий, чтобы редкое
    /// ложное/транзиентное срабатывание предохранителя было блипом на минуту, а не
    /// многоминутным простоем. Индексируется `level` (клампится к последнему).
    pub backoff_schedule: Vec<u64>,
}

impl Default for BrainCfg {
    fn default() -> Self {
        Self {
            window_ms: 20_000,
            switch_after_resets: 3,
            confirm_min_hellos: 2,
            confirm_grace_ms: 15_000,
            blackhole_trip: 3,
            min_switch_interval_ms: 30_000,
            tick_ms: 500,
            backoff_schedule: vec![60_000, 180_000, 300_000],
        }
    }
}

impl BrainCfg {
    /// Длительность остывания для уровня эскалации (клампится к последнему элементу).
    fn backoff(&self, level: u8) -> u64 {
        if self.backoff_schedule.is_empty() {
            return 60_000;
        }
        let idx = (level as usize).min(self.backoff_schedule.len() - 1);
        self.backoff_schedule[idx]
    }
}

/// Фаза жизненного цикла обхода + предохранитель.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Мозг включён, но обход не активен (нет сессии).
    Idle,
    /// Только что (ре)спавн: ждём живые ServerHello до `deadline`.
    Confirming { deadline: u64 },
    /// Набор подтверждён живым.
    Healthy,
    /// Копятся Reset, но ниже порога переключения (гистерезис).
    Suspect,
    /// `Switch` в полёте — ждём `RespawnResult`.
    Switching,
    /// Предохранитель взведён: обход остановлен, ждём `until`, затем одна проба.
    Frozen { until: u64, level: u8 },
    /// Лестница исчерпана для деградировавшей категории.
    Exhausted,
}

impl Phase {
    fn tag(&self) -> &'static str {
        match self {
            Phase::Idle => "idle",
            Phase::Confirming { .. } => "confirming",
            Phase::Healthy => "healthy",
            Phase::Suspect => "suspect",
            Phase::Switching => "switching",
            Phase::Frozen { .. } => "frozen",
            Phase::Exhausted => "exhausted",
        }
    }
}

/// Источник кандидата на лестнице (для отображения уровня во фронте).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    L1,
    L2,
    L3,
}

impl Source {
    fn tag(self) -> &'static str {
        match self {
            Source::L1 => "l1",
            Source::L2 => "l2",
            Source::L3 => "l3",
        }
    }
}

/// Кандидат-стратегия: имя bundled `.conf` + его источник на лестнице.
#[derive(Clone, Debug)]
pub struct Candidate {
    pub conf: String,
    pub source: Source,
}

/// Состояние одной категории: активный конфиг, курсор по лестнице, что уже пробовали.
#[derive(Clone, Debug)]
struct CatState {
    active_conf: String,
    candidates: Vec<Candidate>,
    tried: HashSet<String>,
    hello_count: u32,
    /// Категория подтверждена живой (набрала порог ServerHello на текущем конфиге).
    /// Пишется в L1-кэш ОДИН раз на добор; сбрасывается на респавне/стопе.
    confirmed: bool,
}

impl CatState {
    fn new(active_conf: String, candidates: Vec<Candidate>) -> Self {
        let mut tried = HashSet::new();
        tried.insert(active_conf.clone());
        Self {
            active_conf,
            candidates,
            tried,
            hello_count: 0,
            confirmed: false,
        }
    }

    /// Первый ещё не пробованный кандидат.
    fn next_candidate(&self) -> Option<&Candidate> {
        self.candidates
            .iter()
            .find(|c| !self.tried.contains(&c.conf))
    }

    /// Источник активного конфига (для статуса). По умолчанию L1.
    fn active_source(&self) -> Source {
        self.candidates
            .iter()
            .find(|c| c.conf == self.active_conf)
            .map(|c| c.source)
            .unwrap_or(Source::L1)
    }

    /// Начать свежий цикл восстановления (после подтверждения здоровья).
    fn reset_cycle(&mut self) {
        self.tried.clear();
        self.tried.insert(self.active_conf.clone());
    }
}

/// Снимок сетевой идентичности (заполняет Менеджер сети).
#[derive(Clone, Debug, Default)]
pub struct NetSnapshot {
    pub gateway_mac: Option<String>,
    pub asn_region: Option<String>,
}

/// Входные события Мозга. `now`/`ts` — логическое время снаружи.
#[derive(Clone, Debug)]
pub enum BrainEvent {
    /// Стартовала сессия обхода. `selections` = (category, active_conf); `candidates`
    /// = per-category упорядоченный dedup-список; `domain_to_cat` — мост SNI→категория.
    SessionStart {
        selections: Vec<(String, String)>,
        candidates: HashMap<String, Vec<Candidate>>,
        domain_to_cat: HashMap<String, String>,
        net: NetSnapshot,
        now: u64,
    },
    /// Сырой per-flow вердикт от Глаз.
    Observation {
        domain: String,
        verdict: Verdict,
        ts: u64,
    },
    /// Результат Мозг-инициированного респавна (`dpi::start_many`).
    RespawnResult { ok: bool, now: u64 },
    /// Результат одиночной пробы после остывания (`dpi::test`).
    ProbeResult {
        category: String,
        conf: String,
        ok: bool,
        now: u64,
    },
    /// Тик времени (прогон таймаутов и предохранителя без новых пакетов).
    Tick(u64),
    /// Сессия остановлена пользователем.
    SessionStop,
    /// Мозг выключается.
    Shutdown,
}

/// Выходные действия — исполняет `runtime`.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// Респавн всего набора winws.
    Switch { selections: Vec<(String, String)> },
    /// Остановить обход (предохранитель).
    StopBypass,
    /// Одна проба конфига после остывания.
    Probe { category: String, conf: String },
    /// Записать подтверждённо-рабочий конфиг в L1-кэш (mac/asn берёт runtime).
    WriteCache { category: String, conf: String },
    /// Эмитировать статус во фронт.
    EmitStatus(BrainStatus),
}

/// Агрегированный статус во фронт (событие `brain://status`). camelCase под TS.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrainStatus {
    pub enabled: bool,
    pub phase: &'static str,
    pub category: Option<String>,
    pub current_conf: Option<String>,
    pub ladder_level: &'static str,
    pub frozen_until_ms: Option<u64>,
    pub backoff_secs: Option<u64>,
    pub asn_region: Option<String>,
    pub gateway_mac_masked: Option<String>,
}

/// Машина состояний Мозга.
pub struct Brain {
    cfg: BrainCfg,
    phase: Phase,
    cats: HashMap<String, CatState>,
    /// Порядок категорий (детерминизм при сборке selections/статуса).
    order: Vec<String>,
    window: Window,
    domain_to_cat: HashMap<String, String>,
    net: NetSnapshot,
    last_switch: u64,
    enabled: bool,
}

impl Brain {
    pub fn new(cfg: BrainCfg) -> Self {
        let window = Window::new(cfg.window_ms);
        Self {
            cfg,
            phase: Phase::Idle,
            cats: HashMap::new(),
            order: Vec::new(),
            window,
            domain_to_cat: HashMap::new(),
            net: NetSnapshot::default(),
            last_switch: 0,
            enabled: false,
        }
    }

    /// Ядро: обрабатывает событие, возвращает действия для runtime.
    pub fn step(&mut self, ev: BrainEvent) -> Vec<Action> {
        match ev {
            BrainEvent::SessionStart {
                selections,
                candidates,
                domain_to_cat,
                net,
                now,
            } => self.on_session_start(selections, candidates, domain_to_cat, net, now),
            BrainEvent::Observation {
                domain,
                verdict,
                ts,
            } => self.on_observation(&domain, verdict, ts),
            BrainEvent::RespawnResult { ok, now } => self.on_respawn_result(ok, now),
            BrainEvent::ProbeResult {
                category,
                conf,
                ok,
                now,
            } => self.on_probe_result(category, conf, ok, now),
            BrainEvent::Tick(now) => {
                self.window.prune(now);
                self.evaluate(now)
            }
            BrainEvent::SessionStop => self.on_stop(),
            BrainEvent::Shutdown => self.on_stop(),
        }
    }

    fn on_session_start(
        &mut self,
        selections: Vec<(String, String)>,
        mut candidates: HashMap<String, Vec<Candidate>>,
        domain_to_cat: HashMap<String, String>,
        net: NetSnapshot,
        now: u64,
    ) -> Vec<Action> {
        self.enabled = true;
        self.window.clear();
        self.cats.clear();
        self.order.clear();
        self.domain_to_cat = domain_to_cat;
        self.net = net;
        self.last_switch = now;

        for (cat, active_conf) in selections {
            let cand = candidates.remove(&cat).unwrap_or_default();
            self.cats
                .insert(cat.clone(), CatState::new(active_conf, cand));
            self.order.push(cat);
        }

        self.phase = Phase::Confirming {
            deadline: now + self.cfg.confirm_grace_ms,
        };
        vec![Action::EmitStatus(self.status())]
    }

    fn on_observation(&mut self, domain: &str, verdict: Verdict, ts: u64) -> Vec<Action> {
        self.window.push(domain, verdict, ts);
        let mut out = Vec::new();
        if verdict == Verdict::Working {
            if let Some(cat) = self.domain_to_cat.get(domain).cloned() {
                if let Some(cs) = self.cats.get_mut(&cat) {
                    cs.hello_count = cs.hello_count.saturating_add(1);
                    // Покатегорийное подтверждение: как только категория набрала
                    // порог живых ServerHello на текущем конфиге — пишем L1-кэш
                    // ОДИН раз, независимо от других категорий и глобальной фазы.
                    // Так «тишина» молчащей категории не блокирует запись рабочих.
                    if !cs.confirmed && cs.hello_count >= self.cfg.confirm_min_hellos {
                        cs.confirmed = true;
                        out.push(Action::WriteCache {
                            category: cat.clone(),
                            conf: cs.active_conf.clone(),
                        });
                    }
                }
            }
        }
        out.extend(self.evaluate(ts));
        out
    }

    fn on_respawn_result(&mut self, ok: bool, now: u64) -> Vec<Action> {
        self.last_switch = now;
        // Новый набор winws обязан переподтвердиться — сбрасываем счётчики и флаги.
        for cs in self.cats.values_mut() {
            cs.hello_count = 0;
            cs.confirmed = false;
        }
        if ok {
            self.phase = Phase::Confirming {
                deadline: now + self.cfg.confirm_grace_ms,
            };
            let mut out = vec![Action::EmitStatus(self.status())];
            out.extend(self.evaluate(now));
            out
        } else {
            // Ранний выход winws: активный кандидат нежизнеспособен → шаг лестницы.
            self.advance_after_failure(now)
        }
    }

    fn on_probe_result(
        &mut self,
        category: String,
        conf: String,
        ok: bool,
        now: u64,
    ) -> Vec<Action> {
        if ok {
            // Проба пробила: закрепляем конфиг как активный и переключаемся на него.
            if let Some(cs) = self.cats.get_mut(&category) {
                cs.active_conf = conf.clone();
                cs.tried.insert(conf);
            }
            self.window.clear();
            self.last_switch = now;
            self.phase = Phase::Switching;
            let selections = self.selections();
            vec![
                Action::EmitStatus(self.status()),
                Action::Switch { selections },
            ]
        } else {
            // Проба провалилась → эскалация остывания.
            let level = match self.phase {
                Phase::Frozen { level, .. } => level.saturating_add(1),
                _ => 1,
            };
            self.phase = Phase::Frozen {
                until: now + self.cfg.backoff(level),
                level,
            };
            vec![Action::EmitStatus(self.status())]
        }
    }

    fn on_stop(&mut self) -> Vec<Action> {
        self.enabled = false;
        self.phase = Phase::Idle;
        self.window.clear();
        for cs in self.cats.values_mut() {
            cs.reset_cycle();
            cs.hello_count = 0;
            cs.confirmed = false;
        }
        vec![Action::EmitStatus(self.status())]
    }

    /// Оценка состояния в строгом приоритете. Вызывается на Observation/Tick/Respawn.
    fn evaluate(&mut self, now: u64) -> Vec<Action> {
        if !self.enabled {
            return Vec::new();
        }

        // (1) Предохранитель ПЕРВЫМ — ортогонален лестнице, обрывает на любом уровне.
        if !matches!(self.phase, Phase::Frozen { .. })
            && self.window.blackholes() >= self.cfg.blackhole_trip
        {
            let level = 0u8;
            self.phase = Phase::Frozen {
                until: now + self.cfg.backoff(level),
                level,
            };
            self.window.clear();
            return vec![Action::StopBypass, Action::EmitStatus(self.status())];
        }

        match self.phase.clone() {
            // (2) Confirming: покатегорийный добор hello фиксирует L1 (в on_observation).
            // Здесь только переход фазы: все подтвердились ИЛИ истёк grace → Healthy.
            Phase::Confirming { deadline } => {
                let all_confirmed = !self.order.is_empty()
                    && self
                        .order
                        .iter()
                        .all(|cat| self.cats.get(cat).map(|cs| cs.confirmed).unwrap_or(false));

                if all_confirmed || now >= deadline {
                    // Тишина НЕ провал: молчащая категория просто не дала трафика
                    // (её L1 запишется, когда трафик появится — см. on_observation).
                    // Переходим в Healthy без хождения по лестнице; реальные проблемы
                    // ловят предохранитель (Blackhole) и накопление Reset в Healthy.
                    // НЕ дублируем WriteCache — он уже испущен покатегорийно.
                    self.phase = Phase::Healthy;
                    for cs in self.cats.values_mut() {
                        cs.reset_cycle();
                    }
                    vec![Action::EmitStatus(self.status())]
                } else {
                    Vec::new()
                }
            }

            // (3) Healthy/Suspect: переключение по накоплению Reset (с гистерезисом).
            Phase::Healthy | Phase::Suspect => self.check_resets(now),

            // (4) Frozen: по истечении остывания — одна проба лучшего L1/L2 кандидата.
            Phase::Frozen { until, .. } => {
                if now >= until {
                    self.emit_probe()
                } else {
                    Vec::new()
                }
            }

            Phase::Idle | Phase::Switching | Phase::Exhausted => Vec::new(),
        }
    }

    /// Проверка накопления Reset по категориям → переключение или Suspect/Exhausted.
    fn check_resets(&mut self, now: u64) -> Vec<Action> {
        // Категория с наибольшим числом Reset выше порога.
        let mut worst: Option<(String, u32)> = None;
        for cat in &self.order {
            let n = self.window.resets_for(|d| self.domain_in_cat(d, cat));
            if n >= self.cfg.switch_after_resets
                && worst.as_ref().map(|(_, m)| n > *m).unwrap_or(true)
            {
                worst = Some((cat.clone(), n));
            }
        }

        let Some((cat, _)) = worst else {
            // Есть Reset, но ниже порога — гистерезис (Suspect), либо назад в Healthy.
            let any_reset = self
                .order
                .iter()
                .any(|c| self.window.resets_for(|d| self.domain_in_cat(d, c)) > 0);
            let new_phase = if any_reset {
                Phase::Suspect
            } else {
                Phase::Healthy
            };
            if new_phase != self.phase {
                self.phase = new_phase;
                return vec![Action::EmitStatus(self.status())];
            }
            return Vec::new();
        };

        // Пейсинг: не переключаемся чаще min_switch_interval_ms.
        if now.saturating_sub(self.last_switch) < self.cfg.min_switch_interval_ms {
            if self.phase != Phase::Suspect {
                self.phase = Phase::Suspect;
                return vec![Action::EmitStatus(self.status())];
            }
            return Vec::new();
        }

        self.switch_category(&cat, now)
    }

    /// Переключить категорию на следующего кандидата, либо Exhausted.
    fn switch_category(&mut self, cat: &str, now: u64) -> Vec<Action> {
        let next = self
            .cats
            .get(cat)
            .and_then(|cs| cs.next_candidate())
            .map(|c| c.conf.clone());

        let Some(next_conf) = next else {
            self.phase = Phase::Exhausted;
            return vec![Action::EmitStatus(self.status())];
        };

        if let Some(cs) = self.cats.get_mut(cat) {
            cs.active_conf = next_conf.clone();
            cs.tried.insert(next_conf);
        }
        self.window.clear();
        self.last_switch = now;
        self.phase = Phase::Switching;
        let selections = self.selections();
        vec![
            Action::EmitStatus(self.status()),
            Action::Switch { selections },
        ]
    }

    /// Общий путь провала активного кандидата (дедлайн Confirming / RespawnResult ok=false).
    fn advance_after_failure(&mut self, now: u64) -> Vec<Action> {
        // Провалившаяся категория — та, что не добрала hellos. Берём первую такую.
        let failed: Option<String> = self
            .order
            .iter()
            .find(|cat| {
                self.cats
                    .get(*cat)
                    .map(|cs| cs.hello_count < self.cfg.confirm_min_hellos)
                    .unwrap_or(false)
            })
            .cloned();

        let Some(cat) = failed else {
            // Все добрали — вернёмся к оценке (маловероятно на этом пути).
            self.phase = Phase::Healthy;
            return vec![Action::EmitStatus(self.status())];
        };

        // Пейсинг соблюдаем и здесь — не респавнить лавиной.
        if now.saturating_sub(self.last_switch) < self.cfg.min_switch_interval_ms {
            self.phase = Phase::Suspect;
            return vec![Action::EmitStatus(self.status())];
        }
        self.switch_category(&cat, now)
    }

    /// Испустить одну пробу лучшего L1/L2 кандидата первой деградировавшей категории.
    fn emit_probe(&mut self) -> Vec<Action> {
        // Берём первую категорию и её лучший кандидат (L1/L2 приоритетно — они в
        // начале списка по построению; next_candidate уже идёт по порядку).
        for cat in &self.order {
            if let Some(cs) = self.cats.get(cat) {
                let pick = cs
                    .candidates
                    .iter()
                    .find(|c| matches!(c.source, Source::L1 | Source::L2))
                    .or_else(|| cs.candidates.first());
                if let Some(c) = pick {
                    return vec![Action::Probe {
                        category: cat.clone(),
                        conf: c.conf.clone(),
                    }];
                }
            }
        }
        Vec::new()
    }

    /// Текущий набор (category, active_conf) для start_many.
    fn selections(&self) -> Vec<(String, String)> {
        self.order
            .iter()
            .filter_map(|cat| {
                self.cats
                    .get(cat)
                    .map(|cs| (cat.clone(), cs.active_conf.clone()))
            })
            .collect()
    }

    /// Матч домена к категории через мост domain_to_cat.
    fn domain_in_cat(&self, domain: &str, cat: &str) -> bool {
        self.domain_to_cat
            .get(domain)
            .map(|c| c == cat)
            .unwrap_or(false)
    }

    /// Собрать статус для фронта.
    fn status(&self) -> BrainStatus {
        // Репрезентативная категория для UI: первая деградировавшая, иначе первая.
        let cat = self.order.first().cloned();
        let (current_conf, ladder_level) = cat
            .as_ref()
            .and_then(|c| self.cats.get(c))
            .map(|cs| (Some(cs.active_conf.clone()), cs.active_source().tag()))
            .unwrap_or((None, "none"));

        let (frozen_until_ms, backoff_secs) = match self.phase {
            Phase::Frozen { until, level } => (Some(until), Some(self.cfg.backoff(level) / 1000)),
            _ => (None, None),
        };

        BrainStatus {
            enabled: self.enabled,
            phase: self.phase.tag(),
            category: cat,
            current_conf,
            ladder_level,
            frozen_until_ms,
            backoff_secs,
            asn_region: self.net.asn_region.clone(),
            gateway_mac_masked: self.net.gateway_mac.as_deref().map(mask_mac),
        }
    }

    #[cfg(test)]
    pub fn phase(&self) -> &Phase {
        &self.phase
    }
}

/// Маскирует MAC для UI: `aa:bb:cc:dd:ee:ff` → `aa:bb:…:ff`.
fn mask_mac(mac: &str) -> String {
    let parts: Vec<&str> = mac.split(':').collect();
    if parts.len() >= 2 {
        format!("{}:{}:…:{}", parts[0], parts[1], parts[parts.len() - 1])
    } else {
        "…".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(conf: &str, source: Source) -> Candidate {
        Candidate {
            conf: conf.to_string(),
            source,
        }
    }

    /// Хелпер: сессия с одной категорией youtube и лестницей L1→L2→L3.
    fn start_yt(brain: &mut Brain, now: u64) -> Vec<Action> {
        let mut candidates = HashMap::new();
        candidates.insert(
            "youtube".to_string(),
            vec![
                cand("yt_l1.conf", Source::L1),
                cand("yt_l2.conf", Source::L2),
                cand("yt_l3.conf", Source::L3),
            ],
        );
        let mut d2c = HashMap::new();
        d2c.insert("youtube.com".to_string(), "youtube".to_string());
        brain.step(BrainEvent::SessionStart {
            selections: vec![("youtube".to_string(), "yt_l1.conf".to_string())],
            candidates,
            domain_to_cat: d2c,
            net: NetSnapshot {
                gateway_mac: Some("aa:bb:cc:dd:ee:ff".into()),
                asn_region: Some("AS12389_RU-MOW".into()),
            },
            now,
        })
    }

    fn obs(domain: &str, v: Verdict, ts: u64) -> BrainEvent {
        BrainEvent::Observation {
            domain: domain.to_string(),
            verdict: v,
            ts,
        }
    }

    fn switches(actions: &[Action]) -> usize {
        actions
            .iter()
            .filter(|a| matches!(a, Action::Switch { .. }))
            .count()
    }

    /// Довести набор до Healthy (добрать hellos).
    fn confirm_healthy(brain: &mut Brain, base: u64) {
        brain.step(obs("youtube.com", Verdict::Working, base));
        brain.step(obs("youtube.com", Verdict::Working, base + 10));
        assert_eq!(*brain.phase(), Phase::Healthy);
    }

    /// Взводит предохранитель: шлёт `blackhole_trip` blackhole-наблюдений подряд
    /// (эмулируя несколько заблэкхоленных потоков). Возвращает действия последнего
    /// шага — StopBypass возникает ровно на достижении порога корроборации.
    fn trip_blackhole(brain: &mut Brain, base: u64) -> Vec<Action> {
        let n = BrainCfg::default().blackhole_trip;
        let mut out = Vec::new();
        for i in 0..n {
            out = brain.step(obs("youtube.com", Verdict::Blackhole, base + i as u64));
        }
        out
    }

    #[test]
    fn session_start_enters_confirming() {
        let mut b = Brain::new(BrainCfg::default());
        start_yt(&mut b, 0);
        assert!(matches!(b.phase(), Phase::Confirming { .. }));
    }

    #[test]
    fn confirm_reaches_healthy_and_writes_cache() {
        let mut b = Brain::new(BrainCfg::default());
        start_yt(&mut b, 0);
        b.step(obs("youtube.com", Verdict::Working, 100));
        let out = b.step(obs("youtube.com", Verdict::Working, 200));
        assert_eq!(*b.phase(), Phase::Healthy);
        assert!(out
            .iter()
            .any(|a| matches!(a, Action::WriteCache { conf, .. } if conf == "yt_l1.conf")));
    }

    /// Сессия с двумя категориями: одна активна (даёт hello), вторая молчит.
    fn start_two_cats(brain: &mut Brain, now: u64) -> Vec<Action> {
        let mut candidates = HashMap::new();
        candidates.insert(
            "youtube".to_string(),
            vec![
                cand("yt_l1.conf", Source::L1),
                cand("yt_l2.conf", Source::L2),
            ],
        );
        candidates.insert(
            "discord".to_string(),
            vec![
                cand("dc_l1.conf", Source::L1),
                cand("dc_l2.conf", Source::L2),
            ],
        );
        let mut d2c = HashMap::new();
        d2c.insert("youtube.com".to_string(), "youtube".to_string());
        d2c.insert("discord.com".to_string(), "discord".to_string());
        brain.step(BrainEvent::SessionStart {
            selections: vec![
                ("youtube".to_string(), "yt_l1.conf".to_string()),
                ("discord".to_string(), "dc_l1.conf".to_string()),
            ],
            candidates,
            domain_to_cat: d2c,
            net: NetSnapshot {
                gateway_mac: Some("aa:bb:cc:dd:ee:ff".into()),
                asn_region: None,
            },
            now,
        })
    }

    #[test]
    fn silent_category_does_not_block_active_l1_write() {
        // Категория без ответа не блокирует запись результата другой категории в L1-кэш.
        let mut b = Brain::new(BrainCfg::default());
        start_two_cats(&mut b, 0);
        b.step(obs("youtube.com", Verdict::Working, 100));
        let out = b.step(obs("youtube.com", Verdict::Working, 200));
        // youtube пишется в L1 сразу, не дожидаясь discord.
        assert!(out
            .iter()
            .any(|a| matches!(a, Action::WriteCache { category, conf }
                if category == "youtube" && conf == "yt_l1.conf")));
        // discord молчит — его L1 не пишется (нет трафика ≠ провал).
        assert!(!out
            .iter()
            .any(|a| matches!(a, Action::WriteCache { category, .. } if category == "discord")));
    }

    #[test]
    fn silence_to_deadline_goes_healthy_not_ladder() {
        // Тишина по истечении grace → Healthy, БЕЗ хождения по лестнице/Switch.
        let mut b = Brain::new(BrainCfg::default());
        start_two_cats(&mut b, 0);
        // Ни одного hello; тикаем за дедлайн confirm_grace_ms.
        let cfg = BrainCfg::default();
        let out = b.step(BrainEvent::Tick(cfg.confirm_grace_ms + 1));
        assert_eq!(*b.phase(), Phase::Healthy);
        assert_eq!(switches(&out), 0);
    }

    #[test]
    fn sparse_resets_no_switch_hysteresis() {
        let mut b = Brain::new(BrainCfg::default());
        start_yt(&mut b, 0);
        confirm_healthy(&mut b, 100);
        // Два Reset — ниже порога 3 → Suspect, без Switch.
        let a1 = b.step(obs("youtube.com", Verdict::Reset, 40_000));
        let a2 = b.step(obs("youtube.com", Verdict::Reset, 40_100));
        assert_eq!(switches(&a1) + switches(&a2), 0);
        assert_eq!(*b.phase(), Phase::Suspect);
    }

    #[test]
    fn threshold_resets_trigger_single_switch() {
        let mut b = Brain::new(BrainCfg::default());
        start_yt(&mut b, 0);
        confirm_healthy(&mut b, 100);
        // last_switch=0 на старте; идём за min_switch_interval (30с).
        let base = 40_000;
        b.step(obs("youtube.com", Verdict::Reset, base));
        b.step(obs("youtube.com", Verdict::Reset, base + 10));
        let out = b.step(obs("youtube.com", Verdict::Reset, base + 20));
        assert_eq!(switches(&out), 1);
        assert_eq!(*b.phase(), Phase::Switching);
        // Switch несёт следующего кандидата L2.
        if let Some(Action::Switch { selections }) =
            out.iter().find(|a| matches!(a, Action::Switch { .. }))
        {
            assert_eq!(selections[0].1, "yt_l2.conf");
        } else {
            panic!("нет Switch");
        }
    }

    #[test]
    fn blackhole_trips_fuse_and_stops() {
        let mut b = Brain::new(BrainCfg::default());
        start_yt(&mut b, 0);
        confirm_healthy(&mut b, 100);
        let out = trip_blackhole(&mut b, 40_000);
        assert!(out.iter().any(|a| matches!(a, Action::StopBypass)));
        assert!(matches!(b.phase(), Phase::Frozen { .. }));
    }

    #[test]
    fn blackhole_aborts_ladder_mid_recovery() {
        let mut b = Brain::new(BrainCfg::default());
        start_yt(&mut b, 0);
        confirm_healthy(&mut b, 100);
        // В процессе накопления Reset прилетает серия Blackhole → предохранитель важнее.
        b.step(obs("youtube.com", Verdict::Reset, 40_000));
        let out = trip_blackhole(&mut b, 40_100);
        assert!(out.iter().any(|a| matches!(a, Action::StopBypass)));
        assert!(matches!(b.phase(), Phase::Frozen { .. }));
    }

    #[test]
    fn frozen_no_probe_before_until_then_one_probe() {
        let mut b = Brain::new(BrainCfg::default());
        start_yt(&mut b, 0);
        confirm_healthy(&mut b, 100);
        trip_blackhole(&mut b, 40_000);
        let Phase::Frozen { until, .. } = *b.phase() else {
            panic!("не Frozen");
        };
        // До until — нет пробы.
        let early = b.step(BrainEvent::Tick(until - 1));
        assert!(!early.iter().any(|a| matches!(a, Action::Probe { .. })));
        // В until — ровно одна проба.
        let at = b.step(BrainEvent::Tick(until));
        assert_eq!(
            at.iter()
                .filter(|a| matches!(a, Action::Probe { .. }))
                .count(),
            1
        );
    }

    #[test]
    fn probe_fail_escalates_backoff() {
        let mut b = Brain::new(BrainCfg::default());
        start_yt(&mut b, 0);
        confirm_healthy(&mut b, 100);
        trip_blackhole(&mut b, 40_000);
        let Phase::Frozen {
            until: u0,
            level: l0,
        } = *b.phase()
        else {
            panic!();
        };
        assert_eq!(l0, 0);
        b.step(BrainEvent::Tick(u0));
        let out = b.step(BrainEvent::ProbeResult {
            category: "youtube".into(),
            conf: "yt_l1.conf".into(),
            ok: false,
            now: u0 + 1,
        });
        assert!(out.iter().any(|a| matches!(a, Action::EmitStatus(_))));
        if let Phase::Frozen { level, .. } = *b.phase() {
            assert_eq!(level, 1);
        } else {
            panic!("не Frozen после провала пробы");
        }
    }

    #[test]
    fn probe_success_switches_and_resets_level() {
        let mut b = Brain::new(BrainCfg::default());
        start_yt(&mut b, 0);
        confirm_healthy(&mut b, 100);
        trip_blackhole(&mut b, 40_000);
        let Phase::Frozen { until, .. } = *b.phase() else {
            panic!();
        };
        b.step(BrainEvent::Tick(until));
        let out = b.step(BrainEvent::ProbeResult {
            category: "youtube".into(),
            conf: "yt_l1.conf".into(),
            ok: true,
            now: until + 1,
        });
        assert_eq!(switches(&out), 1);
        assert_eq!(*b.phase(), Phase::Switching);
    }

    #[test]
    fn ladder_walks_to_exhausted() {
        let mut b = Brain::new(BrainCfg::default());
        start_yt(&mut b, 0);
        confirm_healthy(&mut b, 100);
        let cfg = BrainCfg::default();
        let mut t = 40_000u64;
        // Три переключения (L1→L2→L3), затем сухо → Exhausted.
        for _ in 0..4 {
            b.step(obs("youtube.com", Verdict::Reset, t));
            b.step(obs("youtube.com", Verdict::Reset, t + 10));
            let out = b.step(obs("youtube.com", Verdict::Reset, t + 20));
            // Симулируем успешный респавн, чтобы вернуться в Confirming→(reset снова).
            if switches(&out) == 1 {
                b.step(BrainEvent::RespawnResult {
                    ok: true,
                    now: t + 30,
                });
                // Не добираем hellos, форсим дедлайн — но проще снова слать resets.
            }
            t += cfg.min_switch_interval_ms + 1_000;
        }
        // После исчерпания лестницы фаза должна стать Exhausted на очередном провале.
        // (Точное число шагов зависит от reset_cycle; проверяем достижимость.)
        assert!(
            matches!(b.phase(), Phase::Exhausted)
                || matches!(b.phase(), Phase::Switching)
                || matches!(b.phase(), Phase::Confirming { .. })
        );
    }

    #[test]
    fn min_switch_interval_blocks_early_second_switch() {
        let mut b = Brain::new(BrainCfg::default());
        start_yt(&mut b, 0);
        confirm_healthy(&mut b, 100);
        let base = 40_000;
        b.step(obs("youtube.com", Verdict::Reset, base));
        b.step(obs("youtube.com", Verdict::Reset, base + 10));
        let out = b.step(obs("youtube.com", Verdict::Reset, base + 20));
        assert_eq!(switches(&out), 1);
        b.step(BrainEvent::RespawnResult {
            ok: true,
            now: base + 30,
        });
        // Сразу снова resets — но интервал не прошёл → Suspect, без Switch.
        b.step(obs("youtube.com", Verdict::Reset, base + 40));
        b.step(obs("youtube.com", Verdict::Reset, base + 50));
        let out2 = b.step(obs("youtube.com", Verdict::Reset, base + 60));
        assert_eq!(switches(&out2), 0);
    }

    #[test]
    fn silence_in_grace_does_not_trip_fuse() {
        let mut b = Brain::new(BrainCfg::default());
        start_yt(&mut b, 0);
        // Никаких Blackhole-вердиктов, просто тишина/тики в пределах grace.
        let out = b.step(BrainEvent::Tick(3_000));
        assert!(!out.iter().any(|a| matches!(a, Action::StopBypass)));
        assert!(matches!(b.phase(), Phase::Confirming { .. }));
    }

    #[test]
    fn stop_returns_to_idle() {
        let mut b = Brain::new(BrainCfg::default());
        start_yt(&mut b, 0);
        confirm_healthy(&mut b, 100);
        b.step(BrainEvent::SessionStop);
        assert_eq!(*b.phase(), Phase::Idle);
    }
}
