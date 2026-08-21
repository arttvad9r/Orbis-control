//! Правила автоматизации и hardware-inert lifecycle detection.

use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use crate::gpu::GpuMode;
use crate::newtypes::RefreshHz;
use crate::profile::PerformanceProfile;

/// Триггер правила автоматизации.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationTrigger {
    /// Переход на AC.
    OnAc,
    /// Переход на батарею.
    OnBattery,
    /// Слабое USB-C зарядное.
    OnUsbCPdLowPower,
    /// Подключение внешнего дисплея.
    ExternalDisplayConnected,
    /// Отключение внешнего дисплея.
    ExternalDisplayDisconnected,
    /// Выход из сна (resume).
    OnResume,
    /// Закрытие крышки (только через logind).
    LidClosed,
}

/// Действие правила автоматизации.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationAction {
    /// Сменить профиль производительности.
    SetProfile(PerformanceProfile),
    /// Сменить GPU-политику.
    SetGpuPolicy(GpuMode),
    /// Сменить политику частоты экрана (минимум/максимум/auto).
    SetRefreshPolicy(RefreshPolicy),
    /// Выключить/включить подсветку.
    SetLighting(bool),
    /// Пользовательская команда (список аргументов; выполняется только с
    /// явного разрешения пользователя, без /bin/sh -c).
    CustomCommand(Vec<String>),
}

/// Политика частоты обновления экрана.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefreshPolicy {
    /// Минимальная доступная частота (батарея).
    Minimum,
    /// Максимальная доступная частота.
    Maximum,
    /// Авто.
    Auto,
    /// Конкретная частота.
    Fixed(RefreshHz),
}

/// Правило автоматизации.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutomationRule {
    /// Уникальный идентификатор.
    pub id: String,
    /// Триггер.
    pub trigger: AutomationTrigger,
    /// Действие.
    pub action: AutomationAction,
    /// Приоритет (больше = важнее).
    pub priority: u8,
    /// Cooldown между применениями, мс.
    pub cooldown_ms: u64,
    /// Правило включено.
    pub enabled: bool,
}

impl AutomationRule {
    /// Новое правило по умолчанию (cooldown 1500 мс, включено).
    pub fn new(
        id: impl Into<String>,
        trigger: AutomationTrigger,
        action: AutomationAction,
        priority: u8,
    ) -> Self {
        Self {
            id: id.into(),
            trigger,
            action,
            priority,
            cooldown_ms: 1500,
            enabled: true,
        }
    }
}

/// Diagnostic outcome from one power-source observation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PowerSourceObservationOutcome {
    /// A fresh first sample established the baseline; transitions are never
    /// synthesized from process startup state.
    BaselineEstablished,
    /// Fresh sample agrees with the current stable source.
    Stable,
    /// A different fresh source was seen, but more confirmations are required.
    Candidate,
    /// A debounced AC/Battery transition was proven.
    Trigger(AutomationTrigger),
    /// Telemetry did not contain an authoritative AC-online value.
    IgnoredUnknown,
    /// Telemetry sample was older than the configured freshness boundary or had
    /// a timestamp in the future relative to the supplied `now` value.
    IgnoredStale,
}

/// Pure debouncer for authoritative `Telemetry.ac_online` observations.
///
/// The detector performs no I/O. Initial state establishes a baseline and never
/// emits a transition. Unknown/stale samples break candidate continuity so a
/// later edge must again satisfy the full confirmation count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PowerSourceEdgeDetector {
    stable: Option<bool>,
    candidate: Option<bool>,
    candidate_count: u8,
    required_confirmations: u8,
    max_sample_age: Duration,
}

impl PowerSourceEdgeDetector {
    /// Construct a detector. `required_confirmations` must be non-zero.
    pub fn new(required_confirmations: u8, max_sample_age: Duration) -> Option<Self> {
        if required_confirmations == 0 {
            return None;
        }
        Some(Self {
            stable: None,
            candidate: None,
            candidate_count: 0,
            required_confirmations,
            max_sample_age,
        })
    }

    /// Current stable AC state, if a fresh baseline has ever been established.
    pub fn stable_ac_online(&self) -> Option<bool> {
        self.stable
    }

    /// Replace the stable source with a separately proven authoritative value.
    ///
    /// This does not emit a transition. It exists for lifecycle coalescing:
    /// when an enabled `OnResume` reconciliation has already accepted a fresh
    /// post-resume power source, that source becomes the new baseline so the
    /// same change is not emitted again as `OnAc`/`OnBattery` one poll later.
    /// Callers must not use this to bypass the detector's normal freshness or
    /// debounce checks; the supplied value must have been proven elsewhere.
    pub fn rebaseline(&mut self, ac_online: bool) {
        self.stable = Some(ac_online);
        self.reset_candidate();
    }

    /// Break only in-progress debounce continuity while retaining the last
    /// stable source.
    ///
    /// Suspend/resume is a sampling discontinuity: a candidate observed before
    /// sleep must not count as the first of two consecutive fresh confirmations
    /// for a post-resume edge. This method emits nothing and does not guess a new
    /// stable source.
    pub fn break_candidate_continuity(&mut self) {
        self.reset_candidate();
    }

    /// Consume one observation with an explicit caller-supplied clock value.
    ///
    /// Supplying `now` keeps freshness behavior deterministic in tests and
    /// prevents this domain helper from owning a timer or telemetry source.
    pub fn observe(
        &mut self,
        ac_online: Option<bool>,
        sample_at: SystemTime,
        now: SystemTime,
    ) -> PowerSourceObservationOutcome {
        let Some(value) = ac_online else {
            self.reset_candidate();
            return PowerSourceObservationOutcome::IgnoredUnknown;
        };

        let Ok(age) = now.duration_since(sample_at) else {
            self.reset_candidate();
            return PowerSourceObservationOutcome::IgnoredStale;
        };
        if age > self.max_sample_age {
            self.reset_candidate();
            return PowerSourceObservationOutcome::IgnoredStale;
        }

        let Some(stable) = self.stable else {
            self.stable = Some(value);
            self.reset_candidate();
            return PowerSourceObservationOutcome::BaselineEstablished;
        };

        if value == stable {
            self.reset_candidate();
            return PowerSourceObservationOutcome::Stable;
        }

        if self.candidate == Some(value) {
            self.candidate_count = self.candidate_count.saturating_add(1);
        } else {
            self.candidate = Some(value);
            self.candidate_count = 1;
        }

        if self.candidate_count < self.required_confirmations {
            return PowerSourceObservationOutcome::Candidate;
        }

        self.stable = Some(value);
        self.reset_candidate();
        PowerSourceObservationOutcome::Trigger(if value {
            AutomationTrigger::OnAc
        } else {
            AutomationTrigger::OnBattery
        })
    }

    fn reset_candidate(&mut self) {
        self.candidate = None;
        self.candidate_count = 0;
    }
}

impl Default for PowerSourceEdgeDetector {
    fn default() -> Self {
        // Production telemetry currently polls at 1 second. Two consecutive
        // fresh samples and a 3-second freshness ceiling avoid one-sample edges
        // while leaving timing ownership outside this domain object.
        Self::new(2, Duration::from_secs(3)).expect("non-zero confirmation constant")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_rule() {
        let r = AutomationRule::new(
            "ac-profile",
            AutomationTrigger::OnAc,
            AutomationAction::SetProfile(PerformanceProfile::Balanced),
            10,
        );
        assert!(r.enabled);
        assert_eq!(r.cooldown_ms, 1500);
    }

    #[test]
    fn refresh_policy_serde() {
        let p = RefreshPolicy::Fixed(RefreshHz::new(60).unwrap());
        let json = serde_json::to_string(&p).unwrap();
        let back: RefreshPolicy = serde_json::from_str(&json).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn first_power_sample_only_establishes_baseline() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let mut detector = PowerSourceEdgeDetector::default();
        assert_eq!(
            detector.observe(Some(true), now, now),
            PowerSourceObservationOutcome::BaselineEstablished
        );
        assert_eq!(detector.stable_ac_online(), Some(true));
    }

    #[test]
    fn power_transition_requires_consecutive_fresh_confirmations() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let mut detector = PowerSourceEdgeDetector::default();
        assert_eq!(
            detector.observe(Some(true), now, now),
            PowerSourceObservationOutcome::BaselineEstablished
        );
        assert_eq!(
            detector.observe(
                Some(false),
                now + Duration::from_secs(1),
                now + Duration::from_secs(1)
            ),
            PowerSourceObservationOutcome::Candidate
        );
        assert_eq!(
            detector.observe(
                Some(false),
                now + Duration::from_secs(2),
                now + Duration::from_secs(2)
            ),
            PowerSourceObservationOutcome::Trigger(AutomationTrigger::OnBattery)
        );
        assert_eq!(detector.stable_ac_online(), Some(false));
    }

    #[test]
    fn explicit_rebaseline_consumes_pending_edge_without_emitting_transition() {
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let mut detector = PowerSourceEdgeDetector::default();
        detector.observe(Some(true), base, base);
        assert_eq!(
            detector.observe(
                Some(false),
                base + Duration::from_secs(1),
                base + Duration::from_secs(1),
            ),
            PowerSourceObservationOutcome::Candidate
        );

        detector.rebaseline(false);
        assert_eq!(detector.stable_ac_online(), Some(false));
        assert_eq!(
            detector.observe(
                Some(false),
                base + Duration::from_secs(2),
                base + Duration::from_secs(2),
            ),
            PowerSourceObservationOutcome::Stable
        );
    }

    #[test]
    fn suspend_boundary_breaks_candidate_but_keeps_stable_source() {
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let mut detector = PowerSourceEdgeDetector::default();
        detector.observe(Some(true), base, base);
        assert_eq!(
            detector.observe(
                Some(false),
                base + Duration::from_secs(1),
                base + Duration::from_secs(1),
            ),
            PowerSourceObservationOutcome::Candidate
        );

        detector.break_candidate_continuity();
        assert_eq!(detector.stable_ac_online(), Some(true));
        assert_eq!(
            detector.observe(
                Some(false),
                base + Duration::from_secs(10),
                base + Duration::from_secs(10),
            ),
            PowerSourceObservationOutcome::Candidate
        );
        assert_eq!(
            detector.observe(
                Some(false),
                base + Duration::from_secs(11),
                base + Duration::from_secs(11),
            ),
            PowerSourceObservationOutcome::Trigger(AutomationTrigger::OnBattery)
        );
    }

    #[test]
    fn rebaseline_does_not_emit_and_future_real_edge_still_debounces_normally() {
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let mut detector = PowerSourceEdgeDetector::default();
        detector.rebaseline(false);

        assert_eq!(
            detector.observe(
                Some(true),
                base + Duration::from_secs(1),
                base + Duration::from_secs(1),
            ),
            PowerSourceObservationOutcome::Candidate
        );
        assert_eq!(
            detector.observe(
                Some(true),
                base + Duration::from_secs(2),
                base + Duration::from_secs(2),
            ),
            PowerSourceObservationOutcome::Trigger(AutomationTrigger::OnAc)
        );
    }

    #[test]
    fn unknown_or_stale_sample_breaks_candidate_continuity() {
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let mut detector = PowerSourceEdgeDetector::default();
        detector.observe(Some(true), base, base);
        assert_eq!(
            detector.observe(
                Some(false),
                base + Duration::from_secs(1),
                base + Duration::from_secs(1)
            ),
            PowerSourceObservationOutcome::Candidate
        );
        assert_eq!(
            detector.observe(
                None,
                base + Duration::from_secs(2),
                base + Duration::from_secs(2)
            ),
            PowerSourceObservationOutcome::IgnoredUnknown
        );
        assert_eq!(
            detector.observe(
                Some(false),
                base + Duration::from_secs(3),
                base + Duration::from_secs(3)
            ),
            PowerSourceObservationOutcome::Candidate
        );

        // Five seconds old exceeds the default three-second ceiling and resets
        // the candidate again.
        assert_eq!(
            detector.observe(Some(false), base, base + Duration::from_secs(5)),
            PowerSourceObservationOutcome::IgnoredStale
        );
        assert_eq!(
            detector.observe(
                Some(false),
                base + Duration::from_secs(6),
                base + Duration::from_secs(6)
            ),
            PowerSourceObservationOutcome::Candidate
        );
    }

    #[test]
    fn future_sample_never_creates_transition() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let mut detector = PowerSourceEdgeDetector::default();
        detector.observe(Some(true), now, now);
        assert_eq!(
            detector.observe(Some(false), now + Duration::from_secs(1), now),
            PowerSourceObservationOutcome::IgnoredStale
        );
        assert_eq!(detector.stable_ac_online(), Some(true));
    }
}
