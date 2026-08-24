use std::cell::RefCell;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use anyhow::Context;
use orbis_capabilities::CapabilityRegistrySnapshot;
use orbis_config::{AutomationPolicy, load_automation_policy};
use orbis_core::lifecycle::{ResumeGateOutcome, ResumeTelemetryGate};
use orbis_core::telemetry::Telemetry;
use orbis_ui::automation_shadow_runtime::{
    AutomationShadowBlock, AutomationShadowOutcome, AutomationShadowRuntime,
};

// The Automation editing surface left the UI (spec §2.5). What remains here is
// the hardware-inert shadow core: persisted-policy observation, telemetry and
// sleep/resume gates. Execution stays disconnected; nothing here mutates
// hardware or spawns worker commands. Invariant: the shadow never publishes
// runtime-ready=true (execution stays disconnected by construction).

struct AutomationShadowState {
    runtime: AutomationShadowRuntime,
    resume: ResumeTelemetryGate,
    capabilities: Option<Arc<CapabilityRegistrySnapshot>>,
    persisted_policy: Option<AutomationPolicy>,
}

#[derive(Clone)]
struct AutomationContext {
    shadow: Arc<Mutex<AutomationShadowState>>,
}

thread_local! {
    static CONTEXT: RefCell<Option<AutomationContext>> = const { RefCell::new(None) };
}

pub(crate) fn initialize(_runtime: tokio::runtime::Handle) {
    let persisted_policy = match load_policy() {
        Ok(policy) => Some(policy),
        Err(error) => {
            tracing::warn!(error = %error, "Automation shadow policy unavailable at startup");
            None
        }
    };

    CONTEXT.with(|slot| {
        *slot.borrow_mut() = Some(AutomationContext {
            shadow: Arc::new(Mutex::new(AutomationShadowState {
                runtime: AutomationShadowRuntime::default(),
                resume: ResumeTelemetryGate::default(),
                capabilities: None,
                persisted_policy,
            })),
        });
    });
}

pub(crate) fn clear() {
    CONTEXT.with(|slot| *slot.borrow_mut() = None);
}

pub(crate) fn replace_capabilities(snapshot: Arc<CapabilityRegistrySnapshot>) {
    CONTEXT.with(|slot| {
        let Some(context) = slot.borrow().as_ref().cloned() else {
            return;
        };
        match context.shadow.lock() {
            Ok(mut shadow) => shadow.capabilities = Some(snapshot),
            Err(_) => tracing::warn!("Automation shadow state lock poisoned"),
        }
    });
}

/// Feed one paired logind-style sleep/resume observation into the pure resume
/// gate. No plan is created until a later fresh post-resume telemetry sample is
/// observed.
pub(crate) fn observe_prepare_for_sleep(
    start: bool,
    observed_at: SystemTime,
) -> Option<ResumeGateOutcome> {
    let context = CONTEXT.with(|slot| slot.borrow().clone())?;
    match context.shadow.lock() {
        Ok(mut shadow) => Some(shadow.resume.observe_prepare_for_sleep(start, observed_at)),
        Err(_) => {
            tracing::warn!("Automation shadow state lock poisoned");
            None
        }
    }
}

/// Feed one successful authoritative telemetry snapshot into the hardware-inert
/// Automation shadow runtime.
///
/// Only the last successfully loaded/saved policy is considered. Unsaved UI
/// draft changes are deliberately invisible here. The function evaluates both
/// AC/Battery edge detection and a pending paired resume. Neither path invokes a
/// worker/provider setter; a user-facing notice is returned only for confirmed
/// lifecycle events that reached freshness/preflight evaluation.
pub(crate) fn observe_telemetry(telemetry: &Telemetry) -> Option<String> {
    let context = CONTEXT.with(|slot| slot.borrow().clone())?;
    let (power_outcome, resume_gate_outcome, resume_outcome) = {
        let mut shadow = match context.shadow.lock() {
            Ok(shadow) => shadow,
            Err(_) => {
                tracing::warn!("Automation shadow state lock poisoned");
                return Some("Automation shadow runtime unavailable".to_string());
            }
        };
        let policy = shadow.persisted_policy.clone()?;
        let capabilities = shadow.capabilities.clone()?;
        let now = SystemTime::now();

        let power_outcome =
            shadow
                .runtime
                .observe_telemetry(telemetry, &policy, capabilities.as_ref(), now);
        let resume_gate_outcome =
            shadow
                .resume
                .observe_telemetry(telemetry.ac_online, telemetry.ts, now);
        let resume_outcome = if matches!(resume_gate_outcome, ResumeGateOutcome::Ready { .. }) {
            Some(shadow.runtime.observe_resume_telemetry(
                telemetry,
                &policy,
                capabilities.as_ref(),
                now,
            ))
        } else {
            None
        };
        (power_outcome, resume_gate_outcome, resume_outcome)
    };

    tracing::debug!(outcome = ?resume_gate_outcome, "Automation resume telemetry gate");

    if let Some(resume_outcome) = resume_outcome {
        let notice = shadow_notice(&resume_outcome);
        if let Some(ref notice) = notice {
            tracing::info!(outcome = ?resume_outcome, "Automation shadow resume: {notice}");
            return Some(notice.clone());
        }
    }

    let notice = shadow_notice(&power_outcome);
    if let Some(ref notice) = notice {
        tracing::info!(outcome = ?power_outcome, "Automation shadow transition: {notice}");
    }
    notice
}

fn shadow_notice(outcome: &AutomationShadowOutcome) -> Option<String> {
    match outcome {
        AutomationShadowOutcome::Observation(_) => None,
        AutomationShadowOutcome::Blocked {
            trigger,
            generation,
            block,
        } => {
            let reason = match block {
                AutomationShadowBlock::CapabilitySnapshotStale => "capability snapshot stale",
                AutomationShadowBlock::CapabilitySnapshotFromFuture => {
                    "capability snapshot timestamp invalid"
                }
                AutomationShadowBlock::ResumePowerSourceUnknown => {
                    "post-resume power source unknown"
                }
                AutomationShadowBlock::ResumeTelemetryStale => "post-resume telemetry stale",
                AutomationShadowBlock::ResumeTelemetryFromFuture => {
                    "post-resume telemetry timestamp invalid"
                }
            };
            Some(format!(
                "Shadow runtime · {trigger:?} · blocked: {reason} · generation {generation}"
            ))
        }
        AutomationShadowOutcome::PreflightBlocked {
            trigger,
            generation,
            preflight,
        } => Some(format!(
            "Shadow runtime · {trigger:?} · preflight blocked ({} reason{}) · generation {generation}",
            preflight.blocks.len(),
            if preflight.blocks.len() == 1 { "" } else { "s" }
        )),
        AutomationShadowOutcome::ReadyButExecutionDisabled {
            trigger,
            generation,
            preflight,
        } => Some(format!(
            "Shadow runtime · {trigger:?} · {} action{} ready · execution disabled · generation {generation}",
            preflight.plan.actions.len(),
            if preflight.plan.actions.len() == 1 {
                ""
            } else {
                "s"
            }
        )),
    }
}

fn load_policy() -> anyhow::Result<AutomationPolicy> {
    load_automation_policy().context("load hardened Automation policy")
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbis_core::automation::{AutomationTrigger, PowerSourceObservationOutcome};

    #[test]
    fn observation_only_outcomes_do_not_replace_policy_status() {
        assert_eq!(
            shadow_notice(&AutomationShadowOutcome::Observation(
                PowerSourceObservationOutcome::BaselineEstablished
            )),
            None
        );
    }

    #[test]
    fn resume_block_notice_is_explicit_and_never_claims_execution() {
        let outcome = AutomationShadowOutcome::Blocked {
            trigger: AutomationTrigger::OnResume,
            generation: 8,
            block: AutomationShadowBlock::ResumeTelemetryStale,
        };
        let notice = shadow_notice(&outcome).unwrap();
        assert!(notice.contains("OnResume"));
        assert!(notice.contains("telemetry stale"));
        assert!(!notice.contains("Applied"));
    }

    #[test]
    fn ready_shadow_notice_explicitly_says_execution_disabled() {
        use orbis_config::{AutomationPlan, AutomationPowerSource, AutomationPreflight};
        let outcome = AutomationShadowOutcome::ReadyButExecutionDisabled {
            trigger: AutomationTrigger::OnBattery,
            generation: 7,
            preflight: AutomationPreflight {
                plan: AutomationPlan {
                    trigger: AutomationTrigger::OnBattery,
                    power_source: AutomationPowerSource::Battery,
                    actions: Vec::new(),
                    blocks: Vec::new(),
                    reconcile_only: true,
                    notify_transitions: false,
                },
                blocks: Vec::new(),
            },
        };
        let notice = shadow_notice(&outcome).unwrap();
        assert!(notice.contains("execution disabled"));
        assert!(!notice.contains("Applied"));
    }

    #[test]
    fn persistence_backend_contains_no_reconciliation_or_provider_commands() {
        let source = include_str!("automation_backend.rs");
        let forbidden = [
            ["WorkerCommand::", "Set"].concat(),
            ["set_", "profile("].concat(),
            ["set_", "mode("].concat(),
            ["set_", "refresh_rate("].concat(),
            ["set_", "keyboard_backlight("].concat(),
            ["set_", "fan_curve("].concat(),
        ];
        for needle in forbidden {
            assert!(
                !source.contains(&needle),
                "unexpected execution token: {needle}"
            );
        }
        assert!(source.contains("ResumeTelemetryGate"));
        assert!(source.contains("observe_prepare_for_sleep"));
        assert!(source.contains("ReadyButExecutionDisabled"));
    }
}
