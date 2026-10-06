//! Bounded, directed application trials. Observations are user reports about
//! the whole tested configuration, never automatic transport/DPI evidence.
use super::zapret2_measure::{FunctionEvidence, Outcome, RoundMeasurement};
use super::*;
use std::sync::LazyLock;
use std::time::Instant;
use tokio::sync::mpsc;

pub const VERSION: &str = "zapret2-guided-scenarios-v1";
pub const TIMEOUT: Duration = Duration::from_secs(180);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scenario {
    DiscordStartup,
    DiscordVoice,
    YoutubeVideo,
}

impl Scenario {
    pub fn label(self) -> &'static str {
        match self {
            Self::DiscordStartup => "Запуск Discord",
            Self::DiscordVoice => "Голос Discord",
            Self::YoutubeVideo => "Видео YouTube",
        }
    }
    pub fn allowed(self, category: &str, transport: &str) -> bool {
        match self {
            Self::DiscordStartup | Self::DiscordVoice => {
                category == "discord" && transport == "tls"
            }
            Self::YoutubeVideo => {
                category == "youtube_twitch" && matches!(transport, "tls" | "quic")
            }
        }
    }
    pub fn blocks_application_on_failure(self) -> bool {
        self != Self::DiscordVoice
    }
    pub fn steps(self) -> &'static [Step] {
        match self {
            Self::DiscordStartup => &[
                Step { id: "fresh_client", label: "Свежий запуск Discord", instruction: "Полностью закрой Discord из трея и запусти заново. Дождись главного окна с твоими серверами. Если требуется вход, выполни его только в Discord.", min_wait_ms: 0 },
                Step { id: "channels", label: "Загрузка каналов", instruction: "Открой сервер и текстовый канал. Убедись, что каналы и сообщения загрузились, а клиент не остаётся в состоянии подключения. Отправлять сообщения для теста не требуется.", min_wait_ms: 0 },
            ],
            Self::DiscordVoice => &[
                Step { id: "voice_join", label: "Подключение к голосу", instruction: "Полностью перезапусти Discord. Подключись к голосовому каналу, которым обычно пользуешься. Дождись статуса подключения. Используется текущий голосовой профиль вместе с TLS-финалистом.", min_wait_ms: 0 },
                Step { id: "voice_audio", label: "Звук в обе стороны", instruction: "Проверь, что слышишь собеседника и он слышит тебя. Если проверить обе стороны сейчас невозможно, выбери «Не удалось проверить». Один статус подключения не подтверждает звук.", min_wait_ms: 0 },
            ],
            Self::YoutubeVideo => &[
                Step { id: "fresh_video", label: "Запуск публичного видео", instruction: "Открой новую приватную вкладку браузера и видео https://www.youtube.com/watch?v=jNQXAC9IVRw . Дождись начала воспроизведения именно видео, а не рекламы. Не используй уже открытый плеер.", min_wait_ms: 0 },
                Step { id: "playback", label: "Воспроизведение", instruction: "Смотри не менее 10 секунд. Видео и звук должны воспроизводиться без постоянной буферизации. Подтверждение станет доступно через 10 секунд после начала этого шага.", min_wait_ms: 10_000 },
                Step { id: "seek", label: "Продолжение после перемотки", instruction: "Перемотай на другой участок видео и дождись продолжения воспроизведения. Эта проверка относится ко всей конфигурации; фактический TLS/QUIC-транспорт браузера здесь не определяется.", min_wait_ms: 0 },
            ],
        }
    }
}

#[derive(Clone, Copy)]
pub struct Step {
    pub id: &'static str,
    pub label: &'static str,
    pub instruction: &'static str,
    pub min_wait_ms: u64,
}

#[derive(Clone, Copy, Debug, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Observation {
    Passed,
    Failed,
    Inconclusive,
}
impl From<Observation> for Outcome {
    fn from(value: Observation) -> Self {
        match value {
            Observation::Passed => Self::Passed,
            Observation::Failed => Self::Failed,
            Observation::Inconclusive => Self::Inconclusive,
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub request_id: String,
    pub trial_id: String,
    pub run_id: String,
    pub category: String,
    pub transport: String,
    pub profile_id: String,
    pub scenario: Scenario,
    pub phase: &'static str,
    pub step_id: Option<&'static str>,
    pub step_index: usize,
    pub total: usize,
    pub label: &'static str,
    pub instruction: &'static str,
    pub confirm_after_unix_ms: u64,
    pub expires_unix_ms: u64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResultRecord {
    pub request_id: String,
    pub trial_id: String,
    pub run_id: String,
    pub category: String,
    pub transport: String,
    pub profile_id: String,
    pub scenario: Scenario,
    pub source: &'static str,
    pub scope: &'static str,
    pub actual_transport: &'static str,
    pub configuration_fingerprint: String,
    pub status: &'static str,
    pub restored: bool,
    pub context_valid: bool,
    pub recommendation_available: bool,
    pub failure: Option<String>,
    pub history_warning: Option<String>,
    pub elapsed_ms: u64,
    pub measurement: RoundMeasurement,
}

struct Gate {
    step: &'static str,
    eligible: Instant,
    deadline: Instant,
}
struct Active {
    trial: String,
    gate: Option<Gate>,
    sender: mpsc::Sender<Observation>,
}
static ACTIVE: LazyLock<Mutex<Option<Active>>> = LazyLock::new(|| Mutex::new(None));

pub(crate) fn observe(
    trial_id: &str,
    step_id: &str,
    observation: Observation,
) -> Result<(), String> {
    let mut active = ACTIVE.lock_recover();
    let active = active
        .as_mut()
        .filter(|active| active.trial == trial_id)
        .ok_or("Испытание больше не активно.")?;
    let gate = active
        .gate
        .as_ref()
        .filter(|gate| gate.step == step_id)
        .ok_or("Этот шаг уже завершён или ещё не начался.")?;
    let now = Instant::now();
    if now >= gate.deadline {
        return Err("Время испытания истекло; исходный профиль возвращается.".into());
    }
    if matches!(observation, Observation::Passed) && now < gate.eligible {
        return Err("Сначала дождись указанной продолжительности проверки.".into());
    }
    active
        .sender
        .try_send(observation)
        .map_err(|_| "Наблюдение не принято: шаг уже завершён.".to_owned())?;
    active.gate = None;
    Ok(())
}

struct Session {
    trial: String,
    receiver: mpsc::Receiver<Observation>,
}
impl Session {
    fn new(trial: String) -> Result<Self, String> {
        let mut active = ACTIVE.lock_recover();
        if active.is_some() {
            return Err("Другое испытание ещё не завершено.".into());
        }
        let (sender, receiver) = mpsc::channel(1);
        *active = Some(Active {
            trial: trial.clone(),
            gate: None,
            sender,
        });
        Ok(Self { trial, receiver })
    }
    fn arm(&self, step: Step, deadline: Instant) {
        if let Some(active) = ACTIVE
            .lock_recover()
            .as_mut()
            .filter(|active| active.trial == self.trial)
        {
            active.gate = Some(Gate {
                step: step.id,
                eligible: Instant::now() + Duration::from_millis(step.min_wait_ms),
                deadline,
            });
        }
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        let mut active = ACTIVE.lock_recover();
        if active
            .as_ref()
            .is_some_and(|active| active.trial == self.trial)
        {
            *active = None;
        }
    }
}

pub fn pending(round: &mut RoundMeasurement, scenario: Scenario) {
    round.functions = scenario
        .steps()
        .iter()
        .map(|step| FunctionEvidence {
            function: step.id.into(),
            label: step.label.into(),
            required: true,
            outcome: Outcome::NotTested,
            proof: "none",
            stage: "scenario",
            reason: Some("Шаг ещё не проверен.".into()),
            target: String::new(),
            remote_address: None,
            ip_family: None,
            elapsed_ms: 0,
            bytes: 0,
            transfer_bytes: 0,
            transfer_ms: None,
            tls_handshakes: vec![],
            http_status: None,
            connections: vec![],
        })
        .collect();
}

pub fn status(
    round: &RoundMeasurement,
    cancelled: bool,
    restored: bool,
    failure: bool,
) -> &'static str {
    if !restored || failure {
        "aborted"
    } else if cancelled {
        "cancelled"
    } else if !round.comparable || round.runtime_generation.is_none() {
        "inconclusive"
    } else if round
        .functions
        .iter()
        .any(|function| function.outcome == Outcome::Failed)
    {
        "failed"
    } else if round.complete() {
        "passed"
    } else {
        "inconclusive"
    }
}

async fn collect<C, E>(
    scenario: Scenario,
    trial: String,
    timeout: Duration,
    cancel: C,
    mut emit: E,
) -> Result<(Vec<FunctionEvidence>, bool), String>
where
    C: std::future::Future<Output = ()>,
    E: FnMut(Step, usize),
{
    let mut session = Session::new(trial)?;
    let deadline = Instant::now() + timeout;
    let timer = tokio::time::sleep(timeout);
    tokio::pin!(timer, cancel);
    let mut functions = Vec::new();
    let mut cancelled = false;
    let mut terminal = None;
    for (index, step) in scenario.steps().iter().copied().enumerate() {
        let started = Instant::now();
        let mut evidence = FunctionEvidence {
            function: step.id.into(),
            label: step.label.into(),
            required: true,
            outcome: Outcome::NotTested,
            proof: "none",
            stage: "scenario",
            reason: Some("Шаг не проверен.".into()),
            target: String::new(),
            remote_address: None,
            ip_family: None,
            elapsed_ms: 0,
            bytes: 0,
            transfer_bytes: 0,
            transfer_ms: None,
            tls_handshakes: vec![],
            http_status: None,
            connections: vec![],
        };
        if terminal.is_none() {
            session.arm(step, deadline);
            emit(step, index);
            let observation = tokio::select! {
                biased;
                _ = &mut cancel => { cancelled = true; terminal = Some(Outcome::Cancelled); None },
                _ = &mut timer => { terminal = Some(Outcome::Inconclusive); None },
                observation = session.receiver.recv() => observation,
            };
            evidence.elapsed_ms = started.elapsed().as_millis() as u64;
            if let Some(observation) = observation {
                evidence.outcome = observation.into();
                evidence.proof = "user_report";
                evidence.reason = match observation {
                    Observation::Passed => None,
                    Observation::Failed => {
                        Some("Пользователь сообщил об отказе этого шага.".into())
                    }
                    Observation::Inconclusive => {
                        Some("Пользователь не смог проверить этот шаг.".into())
                    }
                };
                if !matches!(observation, Observation::Passed) {
                    terminal = Some(Outcome::NotTested);
                }
            } else {
                evidence.outcome = terminal.unwrap_or(Outcome::Inconclusive);
                evidence.reason = Some(
                    if cancelled {
                        "Испытание отменено."
                    } else {
                        "Шаг не завершён в пределах времени испытания."
                    }
                    .into(),
                );
            }
        } else if terminal == Some(Outcome::Cancelled) {
            evidence.outcome = Outcome::Cancelled;
            evidence.reason = Some("Испытание отменено.".into());
        }
        functions.push(evidence);
    }
    Ok((functions, cancelled))
}

pub(super) async fn measure(
    app: &AppHandle,
    epoch: u64,
    scenario: Scenario,
    mut round: RoundMeasurement,
    mut progress: Progress,
    initial_network: &str,
) -> Result<super::legacy_test::TestReport, String> {
    use tauri::Emitter;
    round.network_before = super::zapret2_measure::network_key().await;
    if round.network_before.as_deref() == Some(initial_network) {
        progress.expires_unix_ms = unix_ms() + TIMEOUT.as_millis() as u64;
        let cancel = async {
            while !super::legacy_test::cancelled(app, epoch) {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        };
        let (functions, cancelled) = collect(
            scenario,
            progress.trial_id.clone(),
            TIMEOUT,
            cancel,
            |step, index| {
                progress.phase = "step";
                progress.step_id = Some(step.id);
                progress.step_index = index + 1;
                progress.label = step.label;
                progress.instruction = step.instruction;
                progress.confirm_after_unix_ms = unix_ms() + step.min_wait_ms;
                let _ = app.emit("zapret2-scenario-progress", &progress);
            },
        )
        .await?;
        round.functions = functions;
        round.network_after = super::zapret2_measure::network_key().await;
        super::zapret2_measure::set_context(&mut round, Some(initial_network));
        let checks = round
            .functions
            .iter()
            .map(|evidence| super::legacy_test::ProbeResult {
                url: String::new(),
                passed: evidence.outcome == Outcome::Passed,
                elapsed_ms: evidence.elapsed_ms,
                bytes: 0,
                attempts: u8::from(evidence.proof == "user_report"),
                http_status: None,
                error: evidence.reason.clone(),
            })
            .collect();
        let mut report = super::legacy_test::report(
            checks,
            cancelled || super::legacy_test::cancelled(app, epoch),
        );
        report.measurement = Some(round);
        Ok(report)
    } else {
        round.network_after = round.network_before.clone();
        super::zapret2_measure::set_context(&mut round, Some(initial_network));
        let mut report = super::legacy_test::report(Vec::new(), false);
        report.measurement = Some(round);
        Ok(report)
    }
}

pub fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    static SERIAL: LazyLock<tokio::sync::Mutex<()>> = LazyLock::new(|| tokio::sync::Mutex::new(()));
    #[tokio::test]
    async fn ordered_observations_are_not_transport_evidence_and_cannot_be_replayed() {
        let _serial = SERIAL.lock().await;
        let (functions, cancelled) = collect(
            Scenario::DiscordStartup,
            "trial".into(),
            Duration::from_secs(1),
            std::future::pending(),
            |step, _| {
                assert!(observe("other", step.id, Observation::Passed).is_err());
                assert!(observe("trial", "wrong_step", Observation::Passed).is_err());
                observe("trial", step.id, Observation::Passed).unwrap();
                assert!(observe("trial", step.id, Observation::Passed).is_err());
            },
        )
        .await
        .unwrap();
        assert!(!cancelled);
        assert_eq!(functions.len(), 2);
        assert!(functions.iter().all(|f| f.outcome == Outcome::Passed
            && f.proof == "user_report"
            && f.remote_address.is_none()));
        assert!(observe("trial", "channels", Observation::Passed).is_err());
    }
    #[tokio::test]
    async fn failed_inconclusive_cancelled_and_expired_steps_never_finish_a_scenario() {
        let _serial = SERIAL.lock().await;
        for observation in [Observation::Failed, Observation::Inconclusive] {
            let (functions, _) = collect(
                Scenario::DiscordStartup,
                "trial".into(),
                Duration::from_secs(1),
                std::future::pending(),
                |step, _| {
                    observe("trial", step.id, observation).unwrap();
                },
            )
            .await
            .unwrap();
            assert_ne!(functions[0].outcome, Outcome::Passed);
            assert_eq!(functions[1].outcome, Outcome::NotTested);
        }
        let (functions, cancelled) = collect(
            Scenario::DiscordStartup,
            "trial".into(),
            Duration::from_secs(1),
            std::future::ready(()),
            |_, _| {},
        )
        .await
        .unwrap();
        assert!(cancelled);
        assert!(functions.iter().all(|f| f.outcome == Outcome::Cancelled));
        let (functions, _) = collect(
            Scenario::DiscordStartup,
            "trial".into(),
            Duration::from_millis(1),
            std::future::pending(),
            |_, _| {},
        )
        .await
        .unwrap();
        assert_eq!(functions[0].outcome, Outcome::Inconclusive);
        assert_eq!(functions[1].outcome, Outcome::NotTested);
        assert!(ACTIVE.lock_recover().is_none());
    }
    #[tokio::test]
    async fn playback_cannot_be_confirmed_early_and_unknown_steps_are_rejected() {
        let _serial = SERIAL.lock().await;
        let session = Session::new("video".into()).unwrap();
        session.arm(
            Scenario::YoutubeVideo.steps()[1],
            Instant::now() + Duration::from_secs(1),
        );
        assert!(observe("video", "playback", Observation::Passed).is_err());
        assert!(serde_json::from_str::<Observation>("\"not_tested\"").is_err());
        assert!(Scenario::YoutubeVideo.allowed("youtube_twitch", "quic"));
        assert!(!Scenario::DiscordVoice.allowed("youtube_twitch", "tls"));
        observe("video", "playback", Observation::Inconclusive).unwrap();
    }

    #[test]
    fn application_verdict_requires_known_context_and_confirmed_restore() {
        let mut round = super::super::zapret2_tournament::tests_round("profile", true);
        round.tester_version = VERSION;
        round.phase = "guided_scenario".into();
        round.functions[0].proof = "user_report";
        assert_eq!(status(&round, false, true, false), "passed");
        assert_eq!(status(&round, false, false, false), "aborted");
        assert_eq!(status(&round, false, true, true), "aborted");
        assert_eq!(status(&round, true, true, false), "cancelled");
        round.comparable = false;
        assert_eq!(status(&round, false, true, false), "inconclusive");
        round.comparable = true;
        round.functions[0].outcome = Outcome::Failed;
        assert_eq!(status(&round, false, true, false), "failed");
        assert!(Scenario::DiscordStartup.blocks_application_on_failure());
        assert!(!Scenario::DiscordVoice.blocks_application_on_failure());
    }
}
