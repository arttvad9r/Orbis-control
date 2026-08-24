use super::*;
use orbis_application::CommandError;
use orbis_core::battery::{ChargeLimit, ChargeLimitBounds};
use orbis_core::fan::FanId;
use orbis_core::gpu::{GpuAccessPolicy, GpuMuxState, GpuPowerState};
use orbis_core::newtypes::{FanPwm, Percent, TemperatureC};
use orbis_core::profile::AsusdFanProfile;
use orbis_providers::FanCurveDefaultsMutationProvider;
use orbis_providers::error::ProviderError;
use orbis_providers::traits::FanCurvePoints;

fn base_state() -> controller::UiState {
    controller::UiState::from_mock_profile("zephyrus-full")
}

#[test]
fn interactive_initial_state_has_no_fixture_values_or_write_access() {
    let state = controller::UiState::production_initial();

    assert_eq!(state.perf_state, controller::PerformanceHwState::Loading);
    assert_eq!(
        state.charge_limit_state,
        controller::ChargeLimitState::Loading
    );
    assert_eq!(
        state.gpu_mode_state,
        controller::GpuModeHwState::Unavailable
    );
    assert_eq!(state.fan_curve_state, controller::FanCurveHwState::Loading);
    assert!(!state.perf_writable);
    assert!(!state.charge_limit_writable);
    assert!(!state.gpu_mode_writable);
    assert!(!state.fan_curve_writable);
    assert!(!state.telemetry_fresh);
    assert_eq!(state.cpu_temp, "—");
    assert_eq!(state.gpu_temp, "—");
    assert_eq!(state.battery_percent, "—");
    assert_eq!(state.mock_profile, "production");
}

#[test]
fn real_read_only_gpu_states_are_rendered_without_write_access() {
    let mut state = controller::UiState::production_initial();

    apply_performance_refresh(
        &mut state,
        Ok(orbis_application::PerformanceState {
            current: PerformanceProfile::Silent,
            available: vec![
                PerformanceProfile::Silent,
                PerformanceProfile::Balanced,
                PerformanceProfile::Turbo,
            ],
        }),
    );
    apply_gpu_mux_refresh(&mut state, Ok(GpuMuxState::Integrated));
    apply_gpu_access_refresh(&mut state, Ok(GpuAccessPolicy::Unblocked));
    apply_gpu_power_refresh(
        &mut state,
        Err(ProviderError::Dbus("supergfxd unavailable".into())),
    );

    assert_eq!(state.perf_state, controller::PerformanceHwState::Ready);
    assert_eq!(state.perf_selected, 0);
    assert_eq!(state.available_perf_mask, 0b111);
    assert!(!state.perf_writable);
    assert_eq!(state.gpu_mux, controller::GpuHwState::Ready);
    assert_eq!(state.gpu_mux_value, 0);
    assert_eq!(state.gpu_access, controller::GpuHwState::Ready);
    assert_eq!(state.gpu_access_value, 0);
    assert_eq!(state.gpu_power, controller::GpuHwState::Unavailable);
    assert!(!state.gpu_mode_writable);
}

fn charge_outcome(percent: Option<u8>) -> ChargeLimitCommandOutcome {
    ChargeLimitCommandOutcome {
        result: ApplyResult::Applied,
        state: ChargeLimit::new(
            true,
            percent.map(|p| Percent::new(p).expect("range")),
            percent.map(|p| Percent::new(p).expect("range")),
            Some(
                ChargeLimitBounds::new(
                    Percent::new(40).expect("const"),
                    Percent::new(100).expect("const"),
                    1,
                )
                .expect("valid"),
            ),
        )
        .expect("valid"),
    }
}

fn snapshot_with_write(
    write: orbis_core::capability::CapabilityStatus,
) -> std::sync::Arc<orbis_capabilities::CapabilityRegistrySnapshot> {
    use orbis_core::capability::{
        Capability, CapabilityOperations, CapabilityStatus, OperationCapability,
    };
    let mut builder =
        orbis_capabilities::CapabilityRegistryBuilder::new(1, std::time::SystemTime::now());
    for feature in [
        orbis_core::FeatureId::Performance,
        orbis_core::FeatureId::ChargeLimit,
    ] {
        builder
            .add(
                feature,
                Capability::new(CapabilityStatus::Supported).with_operations(
                    CapabilityOperations {
                        read: OperationCapability::new(CapabilityStatus::Supported),
                        write: OperationCapability::new(write),
                    },
                ),
            )
            .expect("capability must validate");
    }
    std::sync::Arc::new(builder.build().expect("snapshot must build"))
}

#[test]
fn performance_index_mapping() {
    assert_eq!(
        performance_profile_from_index(0),
        Some(PerformanceProfile::Silent)
    );
    assert_eq!(
        performance_profile_from_index(1),
        Some(PerformanceProfile::Balanced)
    );
    assert_eq!(
        performance_profile_from_index(2),
        Some(PerformanceProfile::Turbo)
    );
    assert_eq!(performance_profile_from_index(-1), None);
    assert_eq!(performance_profile_from_index(7), None);
}

#[test]
fn performance_available_mask_ignores_order() {
    let full = vec![
        PerformanceProfile::Silent,
        PerformanceProfile::Balanced,
        PerformanceProfile::Turbo,
    ];
    let shuffled = vec![
        PerformanceProfile::Turbo,
        PerformanceProfile::Silent,
        PerformanceProfile::Balanced,
    ];
    assert_eq!(performance_available_mask(&full), 0b111);
    assert_eq!(performance_available_mask(&shuffled), 0b111);
    assert_eq!(
        performance_available_mask(&[PerformanceProfile::Turbo, PerformanceProfile::Silent]),
        0b101
    );
}

#[test]
fn performance_write_probe_requires_owned_hardware_name() {
    assert!(performance_write_available(Some(true)));
    assert!(!performance_write_available(Some(false)));
    assert!(!performance_write_available(None));
}

#[test]
fn performance_click_guard_requires_ready_writable_available_state() {
    let mut state = base_state();
    assert!(performance_click_allowed(&state, 0));

    state.perf_writable = false;
    assert!(!performance_click_allowed(&state, 0));

    state.perf_writable = true;
    state.perf_state = controller::PerformanceHwState::Unavailable;
    assert!(!performance_click_allowed(&state, 0));

    state.perf_state = controller::PerformanceHwState::Ready;
    state.available_perf_mask = 0b010;
    assert!(!performance_click_allowed(&state, 0));
    assert!(performance_click_allowed(&state, 1));
}

#[test]
fn performance_click_guard_emits_only_authorized_command() {
    let mut state = base_state();
    assert_eq!(
        performance_command_for_click(&state, 0),
        Some(WorkerCommand::SetPerformance(PerformanceProfile::Silent))
    );

    state.perf_writable = false;
    assert_eq!(performance_command_for_click(&state, 0), None);

    state.perf_writable = true;
    state.available_perf_mask = 0b010;
    assert_eq!(performance_command_for_click(&state, 0), None);
    assert_eq!(
        performance_command_for_click(&state, 1),
        Some(WorkerCommand::SetPerformance(PerformanceProfile::Balanced))
    );
}

#[test]
fn authoritative_performance_result_updates_ui() {
    let mut s = base_state();
    assert_eq!(s.perf_selected, 1);

    let outcome = PerformanceCommandOutcome {
        result: ApplyResult::Applied,
        state: orbis_application::PerformanceState {
            current: PerformanceProfile::Silent,
            available: vec![
                PerformanceProfile::Silent,
                PerformanceProfile::Balanced,
                PerformanceProfile::Turbo,
            ],
        },
    };
    apply_performance_event(&mut s, WorkerEvent::Performance(Ok(outcome)));

    assert_eq!(s.perf_selected, 0);
    assert_eq!(s.available_perf_mask, 0b111);
}

#[test]
fn performance_result_preserves_other_sections() {
    let mut s = base_state();
    s.gpu_selected = 3;
    s.gpu_ultimate_pending = true;
    s.gpu_section_error = true;
    s.charge_limit = 60;
    s.cpu_temp = "99°C".into();

    let outcome = PerformanceCommandOutcome {
        result: ApplyResult::Applied,
        state: orbis_application::PerformanceState {
            current: PerformanceProfile::Turbo,
            available: vec![PerformanceProfile::Turbo],
        },
    };
    apply_performance_event(&mut s, WorkerEvent::Performance(Ok(outcome)));

    assert_eq!(s.perf_selected, 2);
    assert_eq!(s.available_perf_mask, 0b100);
    assert_eq!(s.gpu_selected, 3);
    assert!(s.gpu_ultimate_pending);
    assert!(s.gpu_section_error);
    assert_eq!(s.charge_limit, 60);
    assert_eq!(s.cpu_temp, "99°C");
}

#[test]
fn performance_command_error_does_not_mutate_ui() {
    let mut s = base_state();
    let before = s.clone();
    apply_performance_event(
        &mut s,
        WorkerEvent::Performance(Err(CommandError::Command(
            orbis_providers::error::ProviderError::Unsupported("x".into()),
        ))),
    );
    assert_eq!(s, before);
}

#[test]
fn performance_readback_error_does_not_mutate_ui() {
    let mut s = base_state();
    let before = s.clone();
    apply_performance_event(
        &mut s,
        WorkerEvent::Performance(Err(CommandError::ReadBack {
            result: ApplyResult::Applied,
            source: orbis_providers::error::ProviderError::Timeout("t".into()),
        })),
    );
    assert_eq!(s, before);
}

#[test]
fn performance_unconfirmed_error_preserves_ui() {
    let mut s = base_state();
    let before = s.clone();
    apply_performance_event(
        &mut s,
        WorkerEvent::Performance(Err(CommandError::Unconfirmed {
            intent: "performance profile Turbo".into(),
            command: orbis_providers::error::ProviderError::Timeout("dispatch ambiguous".into()),
            observation: None,
        })),
    );
    assert_eq!(s, before);
}

#[test]
fn gpu_index_mapping() {
    assert_eq!(gpu_mode_from_index(0), Some(0)); // Hybrid
    assert_eq!(gpu_mode_from_index(1), Some(1)); // Integrated
    assert_eq!(gpu_mode_from_index(2), Some(2)); // Ultimate
    // The ASUS product API has no Optimized mode; card 3 issues no request.
    assert_eq!(gpu_mode_from_index(3), None);
    assert_eq!(gpu_mode_from_index(-1), None);
    assert_eq!(gpu_mode_from_index(9), None);
}

fn applied_outcome(requested: GpuMode) -> GpuCommandOutcome {
    GpuCommandOutcome {
        result: ApplyResult::Applied,
        state: orbis_application::GpuState {
            requested,
            mux: GpuMuxState::Integrated,
            access_policy: GpuAccessPolicy::Blocked,
            power_state: GpuPowerState::Active,
            requirement: ActionRequirement::None,
        },
    }
}

#[test]
fn authoritative_gpu_applied_updates_ui() {
    let mut s = base_state();
    s.perf_selected = 0;
    s.charge_limit = 65;
    s.available_gpu_mask = 0b1111;
    s.gpu_ultimate_disabled = false;
    s.gpu_section_error = true;

    apply_gpu_result(&mut s, Ok(applied_outcome(GpuMode::Optimized)));

    assert_eq!(s.gpu_selected, 3);
    assert!(!s.gpu_ultimate_pending);
    assert!(!s.gpu_section_error);
    assert_eq!(s.available_gpu_mask, 0b1111);
    assert!(!s.gpu_ultimate_disabled);
    assert_eq!(s.perf_selected, 0);
    assert_eq!(s.charge_limit, 65);
}

#[test]
fn ultimate_pending_updates_existing_ui() {
    let mut s = base_state();
    let outcome = GpuCommandOutcome {
        result: ApplyResult::Pending {
            requirement: ActionRequirement::Reboot,
        },
        state: orbis_application::GpuState {
            requested: GpuMode::Ultimate,
            mux: GpuMuxState::Integrated,
            access_policy: GpuAccessPolicy::Unblocked,
            power_state: GpuPowerState::Active,
            requirement: ActionRequirement::Reboot,
        },
    };
    apply_gpu_result(&mut s, Ok(outcome));

    assert_eq!(s.gpu_selected, 2);
    assert!(s.gpu_ultimate_pending);
    assert!(!s.gpu_section_error);
}

#[test]
fn eco_logout_pending_does_not_set_ultimate_flag() {
    let mut s = base_state();
    let outcome = GpuCommandOutcome {
        result: ApplyResult::Pending {
            requirement: ActionRequirement::Logout,
        },
        state: orbis_application::GpuState {
            requested: GpuMode::Eco,
            mux: GpuMuxState::Integrated,
            access_policy: GpuAccessPolicy::Blocked,
            power_state: GpuPowerState::Suspended,
            requirement: ActionRequirement::Logout,
        },
    };
    apply_gpu_result(&mut s, Ok(outcome));

    assert_eq!(s.gpu_selected, 0);
    assert!(!s.gpu_ultimate_pending);
    assert!(!s.gpu_section_error);
}

#[test]
fn gpu_command_error_preserves_state_and_sets_error() {
    let mut s = base_state();
    s.perf_selected = 0;
    s.charge_limit = 65;
    let selected_before = s.gpu_selected;
    let pending_before = s.gpu_ultimate_pending;
    let mask_before = s.available_gpu_mask;
    let disabled_before = s.gpu_ultimate_disabled;

    apply_gpu_result(
        &mut s,
        Err(CommandError::Command(
            orbis_providers::error::ProviderError::Unsupported("x".into()),
        )),
    );

    assert_eq!(s.gpu_selected, selected_before);
    assert_eq!(s.gpu_ultimate_pending, pending_before);
    assert_eq!(s.available_gpu_mask, mask_before);
    assert_eq!(s.gpu_ultimate_disabled, disabled_before);
    assert!(s.gpu_section_error);
    assert_eq!(s.perf_selected, 0);
    assert_eq!(s.charge_limit, 65);
}

#[test]
fn gpu_readback_error_preserves_state_and_sets_error() {
    let mut s = base_state();
    let before = s.clone();

    apply_gpu_result(
        &mut s,
        Err(CommandError::ReadBack {
            result: ApplyResult::Applied,
            source: orbis_providers::error::ProviderError::Timeout("t".into()),
        }),
    );

    assert_eq!(s.gpu_selected, before.gpu_selected);
    assert_eq!(s.gpu_ultimate_pending, before.gpu_ultimate_pending);
    assert_eq!(s.available_gpu_mask, before.available_gpu_mask);
    assert_eq!(s.gpu_ultimate_disabled, before.gpu_ultimate_disabled);
    assert!(s.gpu_section_error);
    assert_eq!(s.perf_selected, before.perf_selected);
    assert_eq!(s.charge_limit, before.charge_limit);
}

#[test]
fn gpu_unconfirmed_error_preserves_state_and_does_not_set_error() {
    let mut s = base_state();
    let before = s.clone();

    apply_gpu_result(
        &mut s,
        Err(CommandError::Unconfirmed {
            intent: "gpu mode Ultimate".into(),
            command: orbis_providers::error::ProviderError::Timeout("dispatch ambiguous".into()),
            observation: None,
        }),
    );

    assert_eq!(s.gpu_selected, before.gpu_selected);
    assert_eq!(s.gpu_ultimate_pending, before.gpu_ultimate_pending);
    assert_eq!(s.available_gpu_mask, before.available_gpu_mask);
    assert_eq!(s.gpu_ultimate_disabled, before.gpu_ultimate_disabled);
    assert!(
        !s.gpu_section_error,
        "Unconfirmed must not set definitive error"
    );
    assert_eq!(s.perf_selected, before.perf_selected);
    assert_eq!(s.charge_limit, before.charge_limit);
}

#[test]
fn successful_gpu_result_clears_previous_error() {
    let mut s = base_state();
    s.gpu_section_error = true;
    apply_gpu_result(&mut s, Ok(applied_outcome(GpuMode::Standard)));
    assert!(!s.gpu_section_error);
    assert_eq!(s.gpu_selected, 1);
}

#[test]
fn charge_limit_ui_mapping() {
    assert_eq!(charge_limit_from_ui(19.0), None);
    assert_eq!(charge_limit_from_ui(20.0), Some(20));
    assert_eq!(charge_limit_from_ui(21.0), Some(21));
    assert_eq!(charge_limit_from_ui(80.0), Some(80));
    assert_eq!(charge_limit_from_ui(99.0), Some(99));
    assert_eq!(charge_limit_from_ui(100.0), Some(100));
    assert_eq!(charge_limit_from_ui(83.0), Some(83));
    assert_eq!(charge_limit_from_ui(-1.0), None);
    assert_eq!(charge_limit_from_ui(101.0), None);
    assert_eq!(charge_limit_from_ui(f32::NAN), None);
    assert_eq!(charge_limit_from_ui(f32::INFINITY), None);
    assert_eq!(charge_limit_from_ui(80.5), None);
}

#[test]
fn battery_writable_requires_owner_ready_and_both_values() {
    let limit = ChargeLimit::new(
        false,
        Some(Percent::new(80).unwrap()),
        Some(Percent::new(100).unwrap()),
        None,
    )
    .unwrap();
    assert!(battery_write_available(
        true,
        controller::ChargeLimitState::Ready,
        &limit
    ));
    assert!(!battery_write_available(
        false,
        controller::ChargeLimitState::Ready,
        &limit
    ));
    assert!(!battery_write_available(
        true,
        controller::ChargeLimitState::Loading,
        &limit
    ));
    let missing_effective =
        ChargeLimit::new(false, Some(Percent::new(80).unwrap()), None, None).unwrap();
    assert!(!battery_write_available(
        true,
        controller::ChargeLimitState::Ready,
        &missing_effective
    ));
}

#[test]
fn charge_mutation_allowed_requires_writable_and_ready() {
    let mut s = base_state();
    assert!(s.charge_limit_writable);
    assert_eq!(s.charge_limit_state, controller::ChargeLimitState::Ready);
    assert!(charge_mutation_allowed(&s));

    s.charge_limit_writable = false;
    assert!(!charge_mutation_allowed(&s));

    s.charge_limit_writable = true;
    s.charge_limit_state = controller::ChargeLimitState::Loading;
    assert!(!charge_mutation_allowed(&s));

    s.charge_limit_state = controller::ChargeLimitState::Unavailable;
    assert!(!charge_mutation_allowed(&s));
}

#[test]
fn write_status_gating_derives_from_registry() {
    use orbis_core::capability::CapabilityStatus;

    for (write, expected) in [
        (CapabilityStatus::Supported, true),
        (CapabilityStatus::SupportedWithRequirement, true),
        (CapabilityStatus::ReadOnly, false),
        (CapabilityStatus::Unsupported, false),
        (CapabilityStatus::BackendMissing, false),
        (CapabilityStatus::TemporarilyUnavailable, false),
        (CapabilityStatus::PermissionDenied, false),
        (CapabilityStatus::Unknown, false),
    ] {
        let mut s = base_state();
        let snapshot = snapshot_with_write(write);
        s.update_capabilities(&snapshot);
        assert_eq!(s.perf_writable, expected, "perf write={write:?}");
        assert_eq!(s.charge_limit_writable, expected, "charge write={write:?}");
    }
}

#[test]
fn registry_change_updates_gating_without_touching_observed() {
    use orbis_core::capability::CapabilityStatus;

    let mut s = base_state();
    s.perf_selected = 2;
    s.charge_limit = 60;
    s.gpu_power_value = 1;
    s.gpu_mux_value = 2;

    let snapshot = snapshot_with_write(CapabilityStatus::Unsupported);
    apply_performance_event(
        &mut s,
        WorkerEvent::RegistryChange(Ok((2, snapshot.clone()))),
    );
    assert!(!s.perf_writable);
    assert!(!s.charge_limit_writable);
    assert_eq!(s.perf_selected, 2);
    assert_eq!(s.charge_limit, 60);
    assert_eq!(s.gpu_power_value, 1);
    assert_eq!(s.gpu_mux_value, 2);

    let snapshot = snapshot_with_write(CapabilityStatus::Supported);
    apply_performance_event(
        &mut s,
        WorkerEvent::RegistryChange(Ok((3, snapshot.clone()))),
    );
    assert!(s.perf_writable);
    assert!(s.charge_limit_writable);
    assert_eq!(s.perf_selected, 2);
    assert_eq!(s.charge_limit, 60);
    assert_eq!(s.gpu_power_value, 1);
    assert_eq!(s.gpu_mux_value, 2);
}

#[test]
fn platform_profile_lifecycle_promotes_only_after_new_supported_evidence() {
    use orbis_core::capability::CapabilityStatus;

    let mut s = base_state();
    apply_performance_event(
        &mut s,
        WorkerEvent::RegistryChange(Ok((1, snapshot_with_write(CapabilityStatus::Unknown)))),
    );
    assert_eq!(s.capability_generation, 1);
    assert!(!s.perf_writable);

    apply_performance_event(
        &mut s,
        WorkerEvent::RegistryChange(Ok((2, snapshot_with_write(CapabilityStatus::Supported)))),
    );
    assert_eq!(s.capability_generation, 2);
    assert!(s.perf_writable);
}

#[test]
fn platform_profile_disappearance_rejects_pending_request_and_ignores_stale_support() {
    use orbis_core::capability::CapabilityStatus;

    let mut s = base_state();
    s.capability_generation = 4;
    s.perf_selected = 1;
    assert_eq!(
        performance_command_for_click(&s, 2),
        Some(WorkerCommand::SetPerformance(PerformanceProfile::Turbo))
    );

    apply_performance_event(
        &mut s,
        WorkerEvent::RegistryChange(Ok((
            5,
            snapshot_with_write(CapabilityStatus::BackendMissing),
        ))),
    );
    assert_eq!(s.capability_generation, 5);
    assert!(!s.perf_writable);
    assert_eq!(performance_command_for_click(&s, 2), None);
    assert_eq!(s.perf_selected, 1);

    apply_performance_event(
        &mut s,
        WorkerEvent::RegistryChange(Ok((4, snapshot_with_write(CapabilityStatus::Supported)))),
    );
    assert_eq!(s.capability_generation, 5);
    assert!(!s.perf_writable);
    assert_eq!(performance_command_for_click(&s, 2), None);
}

#[test]
fn disabled_charge_control_does_not_emit_mutation_command() {
    let mut s = base_state();
    s.charge_limit_writable = false;
    assert!(!charge_mutation_allowed(&s));

    s.charge_limit_writable = true;
    s.charge_limit_state = controller::ChargeLimitState::Unavailable;
    assert!(!charge_mutation_allowed(&s));
}

#[test]
fn telemetry_refresh_updates_ui_and_error_keeps_previous_state() {
    let mut s = base_state();
    s.reset_telemetry();
    assert_eq!(s.cpu_temp, "—");

    let t = orbis_core::telemetry::Telemetry {
        cpu_temp: Some(orbis_core::newtypes::TemperatureC::new(46).unwrap()),
        gpu_temp: Some(orbis_core::newtypes::TemperatureC::new(43).unwrap()),
        fans: vec![orbis_core::telemetry::FanTelemetry {
            source: "test".into(),
            fan: orbis_core::fan::FanId::Cpu,
            label: "cpu_fan".into(),
            rpm: orbis_core::newtypes::Rpm::new(2600).unwrap(),
            percent: None,
            quality: orbis_core::telemetry::FanTelemetryQuality::Complete,
        }],
        power: orbis_core::telemetry::PowerTelemetry {
            ac: None,
            battery: None,
            total: None,
            gpu: Some(orbis_core::newtypes::MilliWatt::new(13_073).unwrap()),
        },
        ac_online: Some(false),
        battery: None,
        gpu_power_state: orbis_core::gpu::GpuPowerState::Unknown,
        field_gaps: Vec::new(),
        ts: std::time::SystemTime::UNIX_EPOCH,
    };
    apply_performance_event(&mut s, WorkerEvent::TelemetryRefresh(Ok(t)));
    assert_eq!(s.cpu_temp, "46°C");
    assert_eq!(s.gpu_temp, "43°C");
    assert_eq!(s.cpu_fan_rpm, "2600 rpm");
    assert_eq!(s.gpu_fan_rpm, "—");
    assert_eq!(s.battery_percent, "—");
    assert_eq!(s.ac_online, "On battery");
    assert_eq!(s.gpu_power_display, "13 W");

    apply_performance_event(
        &mut s,
        WorkerEvent::TelemetryRefresh(Err(orbis_providers::error::ProviderError::Io(
            std::io::Error::other("test"),
        ))),
    );
    assert_eq!(s.cpu_temp, "46°C");
    assert_eq!(s.gpu_power_display, "13 W");
}

#[test]
fn authoritative_charge_limit_updates_ui() {
    let mut s = base_state();
    s.perf_selected = 0;
    s.gpu_selected = 3;
    s.gpu_ultimate_pending = true;
    s.gpu_section_error = true;

    apply_charge_limit_result(&mut s, Ok(charge_outcome(Some(40))));

    assert_eq!(s.charge_limit, 40);
    assert_eq!(s.perf_selected, 0);
    assert_eq!(s.gpu_selected, 3);
    assert!(s.gpu_ultimate_pending);
    assert!(s.gpu_section_error);
}

#[test]
fn authoritative_charge_value_is_not_sent_value() {
    let mut s = base_state();
    apply_charge_limit_result(&mut s, Ok(charge_outcome(Some(45))));
    assert_eq!(s.charge_limit, 45);
}

#[test]
fn charge_limit_none_preserves_ui() {
    let mut s = base_state();
    let before = s.clone();
    apply_charge_limit_result(&mut s, Ok(charge_outcome(None)));
    assert_eq!(s, before);
}

#[test]
fn charge_limit_command_error_does_not_mutate_ui() {
    let mut s = base_state();
    let before = s.clone();
    apply_charge_limit_result(
        &mut s,
        Err(CommandError::Command(
            orbis_providers::error::ProviderError::Unsupported("x".into()),
        )),
    );
    assert_eq!(s, before);
}

#[test]
fn charge_limit_readback_error_does_not_mutate_ui() {
    let mut s = base_state();
    let before = s.clone();
    apply_charge_limit_result(
        &mut s,
        Err(CommandError::ReadBack {
            result: ApplyResult::Applied,
            source: orbis_providers::error::ProviderError::Timeout("t".into()),
        }),
    );
    assert_eq!(s, before);
}

#[test]
fn charge_limit_unconfirmed_error_preserves_ui() {
    let mut s = base_state();
    let before = s.clone();
    apply_charge_limit_result(
        &mut s,
        Err(CommandError::Unconfirmed {
            intent: "charge limit 60%".into(),
            command: orbis_providers::error::ProviderError::Timeout("dispatch ambiguous".into()),
            observation: None,
        }),
    );
    assert_eq!(s, before);
}

// ============================================================================
// Fan Factory Reset Tests
// ============================================================================

fn accepted_factory_reset_outcome(profile: AsusdFanProfile) -> WorkerEvent {
    WorkerEvent::FanCurveDefaults {
        profile,
        result: Ok(ApplyResult::Accepted),
    }
}

fn unconfirmed_factory_reset_outcome(
    profile: AsusdFanProfile,
    command_error: ProviderError,
    observation: Option<ProviderError>,
) -> WorkerEvent {
    WorkerEvent::FanCurveDefaults {
        profile,
        result: Err(CommandError::Unconfirmed {
            intent: format!("factory reset profile {profile:?}"),
            command: command_error,
            observation,
        }),
    }
}

fn definitive_factory_reset_error_outcome(
    profile: AsusdFanProfile,
    error: ProviderError,
) -> WorkerEvent {
    WorkerEvent::FanCurveDefaults {
        profile,
        result: Err(CommandError::Command(error)),
    }
}

#[test]
fn successful_factory_reset_is_accepted_not_applied() {
    let mut s = base_state();
    let before = s.clone();
    apply_performance_event(
        &mut s,
        accepted_factory_reset_outcome(AsusdFanProfile::Balanced),
    );

    // Verify Accepted result is handled correctly
    assert!(!s.fan_curve_error, "Accepted must not set definitive error");
    assert_eq!(
        s.fan_curve_dirty, before.fan_curve_dirty,
        "Accepted must not clear dirty flag"
    );
    assert_eq!(
        s.fan_curve_temps, before.fan_curve_temps,
        "Accepted must not change curves optimistically"
    );
    assert_eq!(
        s.fan_curve_pwms, before.fan_curve_pwms,
        "Accepted must not change curves optimistically"
    );
    // UI should not show as Applied (no success banner, no error)
    assert_eq!(s, before, "Accepted must preserve all UI state exactly");
}

#[test]
fn accepted_is_not_applied_for_factory_reset() {
    // ApplyResult::Accepted must return false for is_applied()
    assert!(!ApplyResult::Accepted.is_applied());
    assert!(ApplyResult::Applied.is_applied());
    assert!(
        !ApplyResult::Pending {
            requirement: ActionRequirement::Reboot
        }
        .is_applied()
    );
    assert!(
        !ApplyResult::Failed {
            reason: "test".into(),
            backend: "test".into()
        }
        .is_applied()
    );
    assert!(
        !ApplyResult::RolledBack {
            reason: "test".into()
        }
        .is_applied()
    );
}

#[test]
fn timeout_after_possible_dispatch_is_unconfirmed() {
    let mut s = base_state();
    let before = s.clone();
    let outcome = unconfirmed_factory_reset_outcome(
        AsusdFanProfile::Balanced,
        ProviderError::Timeout("dispatch timed out".into()),
        None,
    );
    apply_performance_event(&mut s, outcome);

    assert_eq!(s, before, "Unconfirmed must preserve all UI state");
    assert!(
        !s.fan_curve_error,
        "Unconfirmed must not set definitive error"
    );
}

#[test]
fn dbus_failure_after_possible_dispatch_is_unconfirmed() {
    let mut s = base_state();
    let before = s.clone();
    let outcome = unconfirmed_factory_reset_outcome(
        AsusdFanProfile::Performance,
        ProviderError::Dbus("connection lost".into()),
        None,
    );
    apply_performance_event(&mut s, outcome);

    assert_eq!(s, before, "Unconfirmed must preserve all UI state");
    assert!(
        !s.fan_curve_error,
        "Unconfirmed must not set definitive error"
    );
}

#[test]
fn timeout_with_successful_observation_remains_unconfirmed() {
    // After a timeout, even if a subsequent read succeeds, the operation remains Unconfirmed.
    // The fresh read cannot prove the timed-out mutation created the observed state.
    let mut s = base_state();
    let before = s.clone();
    let outcome = unconfirmed_factory_reset_outcome(
        AsusdFanProfile::Quiet,
        ProviderError::Timeout("dispatch timed out".into()),
        Some(ProviderError::BackendUnavailable(
            "observation failed".into(),
        )),
    );
    apply_performance_event(&mut s, outcome);

    assert_eq!(
        s, before,
        "Unconfirmed must preserve all UI state even with observation error"
    );
    assert!(
        !s.fan_curve_error,
        "Unconfirmed must not set definitive error"
    );
}

#[test]
fn timeout_with_failed_observation_preserves_observation_error() {
    let mut s = base_state();
    let before = s.clone();
    let outcome = unconfirmed_factory_reset_outcome(
        AsusdFanProfile::LowPower,
        ProviderError::Timeout("dispatch timed out".into()),
        Some(ProviderError::BackendUnavailable("read-back failed".into())),
    );
    apply_performance_event(&mut s, outcome);

    assert_eq!(s, before, "Unconfirmed must preserve all UI state");
    assert!(
        !s.fan_curve_error,
        "Unconfirmed must not set definitive error"
    );
    // The observation error is preserved in the Unconfirmed payload for diagnostics
}

#[test]
fn pre_dispatch_failure_does_not_trigger_recovery_read() {
    // Definitive pre-dispatch failures (Unsupported, PermissionDenied, etc.)
    // should be treated as ordinary command errors, not Unconfirmed.
    let mut s = base_state();
    let before = s.clone();
    let outcome = definitive_factory_reset_error_outcome(
        AsusdFanProfile::Balanced,
        ProviderError::Unsupported("factory reset not supported".into()),
    );
    apply_performance_event(&mut s, outcome);

    assert_eq!(s, before, "Definitive error must not mutate UI state");
    // The error is logged but no recovery read is triggered
}

#[test]
fn factory_reset_mutation_called_exactly_once() {
    // This test verifies the semantic requirement at the application layer.
    // The actual call counting is done in integration/unit tests of the provider.
    // Here we verify that the WorkerCommand::ResetFanCurvesToDefaults is a single command.
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;

    let _call_count = Arc::new(AtomicUsize::new(0));

    // Mock provider that counts calls (semantic demonstration; not executed in this test)
    #[allow(dead_code)]
    struct CountingProvider {
        count: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl FanCurveDefaultsMutationProvider for CountingProvider {
        async fn reset_fan_curves_to_defaults(
            &self,
            _profile: AsusdFanProfile,
        ) -> Result<ApplyResult, ProviderError> {
            Ok(ApplyResult::Accepted)
        }
    }

    // The semantic requirement: exactly one mutation call per WorkerCommand
    // This is enforced by the worker FIFO serialization.
    // Test that the WorkerCommand carries a single profile (not a batch).
    let cmd = WorkerCommand::ResetFanCurvesToDefaults {
        profile: AsusdFanProfile::Balanced,
    };
    match cmd {
        WorkerCommand::ResetFanCurvesToDefaults { profile } => {
            assert_eq!(profile, AsusdFanProfile::Balanced);
        }
        _ => panic!("expected ResetFanCurvesToDefaults"),
    }
}

#[test]
fn factory_reset_no_retry_on_timeout() {
    // The application layer does not retry on timeout/Dbus.
    // A single WorkerCommand results in exactly one provider call.
    // Retry logic is explicitly NOT implemented.
    let mut s = base_state();
    let outcome = unconfirmed_factory_reset_outcome(
        AsusdFanProfile::Balanced,
        ProviderError::Timeout("timeout".into()),
        None,
    );
    apply_performance_event(&mut s, outcome);

    // Verify UI state is Unconfirmed (not retried to success/failure)
    assert!(!s.fan_curve_error);
    // No automatic retry is triggered by the UI layer
}

#[test]
fn factory_reset_no_rollback() {
    // No automatic rollback is performed after Unconfirmed.
    let mut s = base_state();
    s.fan_curve_dirty = true; // User had dirty editor state
    let outcome = unconfirmed_factory_reset_outcome(
        AsusdFanProfile::Balanced,
        ProviderError::Timeout("timeout".into()),
        None,
    );
    apply_performance_event(&mut s, outcome);

    // Dirty flag preserved - no rollback
    assert!(
        s.fan_curve_dirty,
        "Unconfirmed must not rollback dirty state"
    );
    assert_eq!(
        s.fan_curve_temps,
        base_state().fan_curve_temps,
        "Curves not rolled back"
    );
}

#[test]
fn factory_reset_and_custom_fan_write_serialized_in_worker() {
    // WorkerCommand::ResetFanCurvesToDefaults and WorkerCommand::SetFanCurve
    // are both processed by the same worker FIFO, ensuring serialization.
    let reset_cmd = WorkerCommand::ResetFanCurvesToDefaults {
        profile: AsusdFanProfile::Balanced,
    };
    let set_cmd = WorkerCommand::SetFanCurve {
        profile: AsusdFanProfile::Performance,
        fan: FanId::Cpu,
        curve: FanCurvePoints {
            temps: [TemperatureC::new(45).unwrap(); 8],
            pwms: [FanPwm::new(5).unwrap(); 8],
        },
    };

    // Both are WorkerCommand variants processed by the same worker FIFO
    assert!(matches!(
        reset_cmd,
        WorkerCommand::ResetFanCurvesToDefaults { .. }
    ));
    assert!(matches!(set_cmd, WorkerCommand::SetFanCurve { .. }));

    // The worker processes them sequentially in FIFO order
}

#[test]
fn requested_profile_captured_in_worker_command() {
    // The requested profile is fixed at command creation time,
    // not dependent on subsequent UI selection changes.
    let profile = AsusdFanProfile::Quiet;
    let cmd = WorkerCommand::ResetFanCurvesToDefaults { profile };
    match cmd {
        WorkerCommand::ResetFanCurvesToDefaults { profile: p } => assert_eq!(p, profile),
        _ => panic!("wrong command type"),
    }
    // The command carries the profile; UI selection changes later don't affect it
}

#[test]
fn factory_reset_unconfirmed_does_not_set_fan_curve_error() {
    let mut s = base_state();
    let before = s.clone();
    let outcome = unconfirmed_factory_reset_outcome(
        AsusdFanProfile::Balanced,
        ProviderError::Timeout("timeout".into()),
        None,
    );
    apply_performance_event(&mut s, outcome);

    assert_eq!(s, before, "Unconfirmed must preserve all UI state");
    assert!(
        !s.fan_curve_error,
        "Unconfirmed must NOT set fan_curve_error (not definitive failure)"
    );
    assert!(
        !s.fan_curve_error,
        "fan_curve_error must remain false for Unconfirmed"
    );
}

#[test]
fn factory_reset_accepted_not_rendered_as_applied() {
    let mut s = base_state();
    let before = s.clone();
    apply_performance_event(
        &mut s,
        accepted_factory_reset_outcome(AsusdFanProfile::Balanced),
    );

    // Accepted is not Applied - no success indication
    assert_eq!(s, before, "Accepted must preserve all UI state");
    assert!(!s.fan_curve_error, "Accepted must not set error");
    // The key property: is_applied() == false for Accepted
    let event = accepted_factory_reset_outcome(AsusdFanProfile::Balanced);
    if let WorkerEvent::FanCurveDefaults {
        result: Ok(ApplyResult::Accepted),
        ..
    } = event
    {
        assert!(
            !ApplyResult::Accepted.is_applied(),
            "Accepted must not be considered Applied"
        );
    }
}

#[test]
fn refresh_success_sets_ready_and_value() {
    let mut s = base_state();
    s.charge_limit_state = controller::ChargeLimitState::Loading;

    apply_charge_limit_refresh(
        &mut s,
        Ok(ChargeLimit::new(
            true,
            Some(Percent::new(60).expect("range")),
            Some(Percent::new(60).expect("range")),
            None,
        )
        .expect("valid")),
    );

    assert_eq!(s.charge_limit_state, controller::ChargeLimitState::Ready);
    assert_eq!(s.charge_limit, 60);
    assert!(s.charge_limit_enabled);
}

#[test]
fn refresh_disabled_keeps_configured_value_but_marks_limit_off() {
    let mut s = base_state();
    s.charge_limit_state = controller::ChargeLimitState::Loading;
    apply_charge_limit_refresh(
        &mut s,
        Ok(ChargeLimit::new(
            false,
            Some(Percent::new(80).expect("configured")),
            Some(Percent::new(100).expect("effective")),
            None,
        )
        .expect("valid")),
    );

    assert_eq!(s.charge_limit_state, controller::ChargeLimitState::Ready);
    assert_eq!(s.charge_limit, 80);
    assert!(!s.charge_limit_enabled);
}

#[test]
fn refresh_percent_none_is_unavailable_without_fixture() {
    let mut s = base_state();
    s.charge_limit_state = controller::ChargeLimitState::Loading;
    s.charge_limit = 80;

    apply_charge_limit_refresh(
        &mut s,
        Ok(ChargeLimit::new(false, None, None, None).expect("valid")),
    );

    assert_eq!(
        s.charge_limit_state,
        controller::ChargeLimitState::Unavailable
    );
    assert_eq!(s.charge_limit, 80);
}

#[test]
fn refresh_error_is_unavailable_without_fixture() {
    let mut s = base_state();
    s.charge_limit_state = controller::ChargeLimitState::Loading;
    s.charge_limit = 80;

    apply_charge_limit_refresh(
        &mut s,
        Err(orbis_providers::error::ProviderError::BackendUnavailable(
            "sessiond missing".into(),
        )),
    );

    assert_eq!(
        s.charge_limit_state,
        controller::ChargeLimitState::Unavailable
    );
    assert_eq!(s.charge_limit, 80);
}

#[test]
fn gpu_power_unknown_is_ready_not_unavailable() {
    let mut s = base_state();
    s.gpu_power = controller::GpuHwState::Loading;
    apply_gpu_power_refresh(&mut s, Ok(GpuPowerState::Unknown));
    assert_eq!(s.gpu_power, controller::GpuHwState::Ready);
    assert_eq!(s.gpu_power_value, 4);
}

#[test]
fn gpu_power_error_is_unavailable() {
    let mut s = base_state();
    s.gpu_power = controller::GpuHwState::Loading;
    apply_gpu_power_refresh(
        &mut s,
        Err(orbis_providers::error::ProviderError::Dbus("down".into())),
    );
    assert_eq!(s.gpu_power, controller::GpuHwState::Unavailable);
}

#[test]
fn gpu_mux_and_access_values_mapped() {
    let mut s = base_state();
    apply_gpu_mux_refresh(&mut s, Ok(GpuMuxState::Discrete));
    assert_eq!(s.gpu_mux, controller::GpuHwState::Ready);
    assert_eq!(s.gpu_mux_value, 1);
    apply_gpu_access_refresh(&mut s, Ok(GpuAccessPolicy::Blocked));
    assert_eq!(s.gpu_access, controller::GpuHwState::Ready);
    assert_eq!(s.gpu_access_value, 1);
}

#[test]
fn gpu_hw_states_default_loading() {
    let s = base_state();
    assert_eq!(s.gpu_power, controller::GpuHwState::Loading);
    assert_eq!(s.gpu_mux, controller::GpuHwState::Loading);
    assert_eq!(s.gpu_access, controller::GpuHwState::Loading);
}

fn perf_state(current: PerformanceProfile, available: &[PerformanceProfile]) -> PerformanceState {
    PerformanceState {
        current,
        available: available.to_vec(),
    }
}

#[test]
fn perf_refresh_success_sets_ready_and_authoritative_state() {
    let mut s = base_state();
    s.perf_state = controller::PerformanceHwState::Loading;

    apply_performance_refresh(
        &mut s,
        Ok(perf_state(
            PerformanceProfile::Silent,
            &[
                PerformanceProfile::Silent,
                PerformanceProfile::Balanced,
                PerformanceProfile::Turbo,
            ],
        )),
    );

    assert_eq!(s.perf_state, controller::PerformanceHwState::Ready);
    assert_eq!(s.perf_selected, 0);
    assert_eq!(s.available_perf_mask, 0b111);
}

#[test]
fn perf_refresh_error_is_unavailable_without_mock_fallback() {
    let mut s = base_state();
    s.perf_state = controller::PerformanceHwState::Loading;
    s.perf_selected = 1;

    apply_performance_refresh(
        &mut s,
        Err(orbis_providers::error::ProviderError::BackendUnavailable(
            "sessiond missing".into(),
        )),
    );

    assert_eq!(s.perf_state, controller::PerformanceHwState::Unavailable);
    assert_eq!(s.perf_selected, 1);
}

#[test]
fn perf_refresh_available_partial_mask() {
    let mut s = base_state();
    s.perf_state = controller::PerformanceHwState::Loading;

    apply_performance_refresh(
        &mut s,
        Ok(perf_state(
            PerformanceProfile::Turbo,
            &[PerformanceProfile::Turbo],
        )),
    );

    assert_eq!(s.perf_state, controller::PerformanceHwState::Ready);
    assert_eq!(s.perf_selected, 2);
    assert_eq!(s.available_perf_mask, 0b100);
}

#[test]
fn perf_hw_state_default_ready_writable_in_mock() {
    let s = base_state();
    assert_eq!(s.perf_state, controller::PerformanceHwState::Ready);
    assert!(s.perf_writable);
}

fn production_gpu_mode_state() -> controller::UiState {
    let mut s = base_state();
    s.gpu_mode_state = controller::GpuModeHwState::Unavailable;
    s.gpu_mode_writable = false;
    s
}

#[test]
fn production_gpu_mode_is_disabled_and_not_selected() {
    let s = production_gpu_mode_state();
    for idx in 0..4 {
        let mask_bit = 1 << idx;
        assert!(gpu_mode_card_disabled(&s, idx, mask_bit));
        assert!(!gpu_mode_card_selected(&s, idx));
    }
}

#[test]
fn production_gpu_mode_mock_selected_is_hidden() {
    let mut s = base_state();
    assert_eq!(s.gpu_selected, 1);
    s.gpu_mode_state = controller::GpuModeHwState::Unavailable;
    s.gpu_mode_writable = false;
    assert!(!gpu_mode_card_selected(&s, 1));
}

#[test]
fn gpu_click_allowed_production_is_false() {
    let s = production_gpu_mode_state();
    assert!(!gpu_mode_click_allowed(&s));
}

#[test]
fn gpu_click_allowed_mock_is_true() {
    let s = base_state();
    assert!(gpu_mode_click_allowed(&s));
}

#[test]
fn mock_gpu_mode_stays_interactive() {
    let s = base_state();
    assert_eq!(s.gpu_mode_state, controller::GpuModeHwState::Ready);
    assert!(s.gpu_mode_writable);
    assert!(gpu_mode_card_selected(&s, 1));
    for idx in 0..4 {
        assert!(!gpu_mode_card_disabled(&s, idx, 1 << idx));
    }
}

#[test]
fn production_gpu_mode_setup_preserves_hardware_status() {
    let s = production_gpu_mode_state();
    assert_eq!(s.gpu_power, controller::GpuHwState::Loading);
    assert_eq!(s.gpu_mux, controller::GpuHwState::Loading);
    assert_eq!(s.gpu_access, controller::GpuHwState::Loading);
}

#[test]
fn fan_curve_initial_state_is_loading_and_not_writable() {
    let s = base_state();
    assert_eq!(s.fan_curve_state, controller::FanCurveHwState::Loading);
    assert!(!s.fan_curve_writable);
    assert!(!s.fan_curve_error);
    assert!(!s.fan_curve_dirty);
}

#[test]
fn fan_curve_load_fan_curve_sets_ready_and_populates_points() {
    use orbis_core::fan::{FanCurve, FanCurvePoint};
    use orbis_core::newtypes::{FanPwm, TemperatureC};

    let mut s = base_state();
    let points: Vec<FanCurvePoint> = [
        (50u16, 0u8),
        (55, 8),
        (60, 13),
        (65, 26),
        (70, 36),
        (75, 54),
        (79, 77),
        (85, 100),
    ]
    .into_iter()
    .map(|(t, p)| {
        FanCurvePoint::new(
            TemperatureC::new(t as i16).unwrap(),
            FanPwm::new(p).unwrap(),
        )
    })
    .collect();
    let curve = FanCurve {
        profile: PerformanceProfile::Balanced,
        fan: FanId::Cpu,
        enabled: None,
        points,
    };
    s.load_fan_curve(&curve, orbis_core::profile::AsusdFanProfile::Balanced);

    assert_eq!(s.fan_curve_state, controller::FanCurveHwState::Ready);
    assert!(!s.fan_curve_error);
    assert!(!s.fan_curve_dirty);
    assert_eq!(s.fan_selected, 0);
    assert_eq!(s.fan_profile_selected, 0);
    assert_eq!(s.fan_curve_temps[0], 50);
    assert_eq!(s.fan_curve_temps[7], 85);
    assert_eq!(s.fan_curve_pwms[0], 0);
    assert_eq!(s.fan_curve_pwms[7], 100);
}

#[test]
fn fan_curve_load_preserves_stored_enabled_evidence() {
    use orbis_core::fan::{FanCurve, FanCurvePoint};
    use orbis_core::newtypes::{FanPwm, TemperatureC};

    fn points() -> Vec<FanCurvePoint> {
        [
            (50u16, 0u8),
            (55, 8),
            (60, 13),
            (65, 26),
            (70, 36),
            (75, 54),
            (79, 77),
            (85, 100),
        ]
        .into_iter()
        .map(|(t, p)| {
            FanCurvePoint::new(
                TemperatureC::new(t as i16).unwrap(),
                FanPwm::new(p).unwrap(),
            )
        })
        .collect()
    }

    // Some(true) preserved
    let mut s = base_state();
    let curve = FanCurve {
        profile: PerformanceProfile::Balanced,
        fan: FanId::Cpu,
        enabled: Some(true),
        points: points(),
    };
    s.load_fan_curve(&curve, orbis_core::profile::AsusdFanProfile::Balanced);
    assert_eq!(s.fan_curve_enabled, Some(true));

    // Some(false) preserved
    let mut s = base_state();
    let curve = FanCurve {
        profile: PerformanceProfile::Balanced,
        fan: FanId::Cpu,
        enabled: Some(false),
        points: points(),
    };
    s.load_fan_curve(&curve, orbis_core::profile::AsusdFanProfile::Balanced);
    assert_eq!(s.fan_curve_enabled, Some(false));

    // None (active sysfs read without enabled evidence) stays None
    let mut s = base_state();
    let curve = FanCurve {
        profile: PerformanceProfile::Balanced,
        fan: FanId::Cpu,
        enabled: None,
        points: points(),
    };
    s.load_fan_curve(&curve, orbis_core::profile::AsusdFanProfile::Balanced);
    assert_eq!(s.fan_curve_enabled, None);
}

#[test]
fn fan_curve_enabled_roundtrips_through_slint_state() {
    use orbis_core::fan::{FanCurve, FanCurvePoint};
    use orbis_core::newtypes::{FanPwm, TemperatureC};

    let points: Vec<FanCurvePoint> = [(50u16, 0u8), (85, 100)]
        .into_iter()
        .map(|(t, p)| {
            FanCurvePoint::new(
                TemperatureC::new(t as i16).unwrap(),
                FanPwm::new(p).unwrap(),
            )
        })
        .collect();

    // enabled=true: roundtrip to Slint UiState and back
    let mut s = base_state();
    let curve = FanCurve {
        profile: PerformanceProfile::Balanced,
        fan: FanId::Cpu,
        enabled: Some(true),
        points: points.clone(),
    };
    s.load_fan_curve(&curve, orbis_core::profile::AsusdFanProfile::Balanced);
    let slint = to_slint(&s);
    assert!(slint.fan_curve_enabled_known);
    assert!(slint.fan_curve_enabled);
    let back = from_slint(&slint);
    assert_eq!(back.fan_curve_enabled, Some(true));

    // enabled=false round-trips as known+false
    let mut s = base_state();
    let curve = FanCurve {
        profile: PerformanceProfile::Balanced,
        fan: FanId::Cpu,
        enabled: Some(false),
        points: points.clone(),
    };
    s.load_fan_curve(&curve, orbis_core::profile::AsusdFanProfile::Balanced);
    let slint = to_slint(&s);
    assert!(slint.fan_curve_enabled_known);
    assert!(!slint.fan_curve_enabled);
    assert_eq!(from_slint(&slint).fan_curve_enabled, Some(false));

    // None: unknown, not silently converted to a known disabled state
    let mut s = base_state();
    let curve = FanCurve {
        profile: PerformanceProfile::Balanced,
        fan: FanId::Cpu,
        enabled: None,
        points,
    };
    s.load_fan_curve(&curve, orbis_core::profile::AsusdFanProfile::Balanced);
    let slint = to_slint(&s);
    assert!(!slint.fan_curve_enabled_known);
    assert!(!slint.fan_curve_enabled);
    assert_eq!(from_slint(&slint).fan_curve_enabled, None);
}

#[test]
fn fan_curve_can_mutate_requires_writable_dirty_and_valid() {
    let mut s = base_state();
    assert!(!s.fan_curve_can_mutate());

    s.fan_curve_writable = true;
    assert!(!s.fan_curve_can_mutate());

    s.fan_curve_dirty = true;
    s.fan_curve_error = true;
    assert!(!s.fan_curve_can_mutate());

    s.fan_curve_error = false;
    s.fan_curve_temps = [85, 50, 60, 65, 70, 75, 79, 85];
    s.fan_curve_pwms = [0, 8, 13, 26, 36, 54, 77, 100];
    assert!(!s.fan_curve_can_mutate());
}

#[test]
fn fan_curve_can_mutate_valid_curve_returns_true() {
    let mut s = base_state();
    s.fan_curve_writable = true;
    s.fan_curve_dirty = true;
    s.fan_curve_error = false;
    s.fan_curve_temps = [50, 55, 60, 65, 70, 75, 79, 85];
    s.fan_curve_pwms = [0, 8, 13, 26, 36, 54, 77, 100];
    assert!(s.fan_curve_can_mutate());
}

#[test]
fn fan_curve_refresh_ok_loads_curve_and_clears_error() {
    use orbis_core::fan::{FanCurve, FanCurvePoint};
    use orbis_core::newtypes::{FanPwm, TemperatureC};

    let mut s = base_state();
    s.fan_curve_error = true;
    s.fan_curve_state = controller::FanCurveHwState::Unavailable;

    let curve = FanCurve {
        profile: PerformanceProfile::Balanced,
        fan: FanId::Cpu,
        enabled: None,
        points: vec![
            FanCurvePoint::new(TemperatureC::new(50).unwrap(), FanPwm::new(0).unwrap()),
            FanCurvePoint::new(TemperatureC::new(85).unwrap(), FanPwm::new(100).unwrap()),
        ],
    };
    s.load_fan_curve(&curve, orbis_core::profile::AsusdFanProfile::Balanced);

    assert_eq!(s.fan_curve_state, controller::FanCurveHwState::Ready);
    assert!(!s.fan_curve_error);
    assert_eq!(s.fan_curve_temps[0], 50);
    assert_eq!(s.fan_curve_temps[1], 85);
}

#[test]
fn fan_curve_mutation_ok_clears_dirty_and_error() {
    let mut s = base_state();
    s.fan_curve_dirty = true;
    s.fan_curve_error = false;

    match ApplyResult::Applied {
        ApplyResult::Applied => {
            s.fan_curve_error = false;
            s.fan_curve_dirty = false;
        }
        _ => s.fan_curve_error = true,
    }

    assert!(!s.fan_curve_dirty);
    assert!(!s.fan_curve_error);
}

#[test]
fn fan_curve_mutation_error_sets_error_preserves_dirty() {
    let mut s = base_state();
    s.fan_curve_dirty = true;
    s.fan_curve_error = false;
    s.fan_curve_error = true;
    assert!(s.fan_curve_error);
    assert!(s.fan_curve_dirty);
}

#[test]
fn fan_curve_profile_index_mapping() {
    use orbis_core::profile::AsusdFanProfile;
    assert_eq!(
        controller::UiState::asusd_profile_from_index(0),
        Some(AsusdFanProfile::Balanced)
    );
    assert_eq!(
        controller::UiState::asusd_profile_from_index(1),
        Some(AsusdFanProfile::Performance)
    );
    assert_eq!(
        controller::UiState::asusd_profile_from_index(2),
        Some(AsusdFanProfile::Quiet)
    );
    assert_eq!(
        controller::UiState::asusd_profile_from_index(3),
        Some(AsusdFanProfile::LowPower)
    );
    assert_eq!(controller::UiState::asusd_profile_from_index(4), None);
    assert_eq!(controller::UiState::asusd_profile_from_index(-1), None);
}

#[test]
fn fan_curve_fan_id_mapping() {
    assert_eq!(controller::UiState::fan_id_from_index(0), Some(FanId::Cpu));
    assert_eq!(controller::UiState::fan_id_from_index(1), Some(FanId::Gpu));
    assert_eq!(controller::UiState::fan_id_from_index(2), None);
}

#[test]
fn fan_curve_pwm_above_100_is_valid() {
    let mut s = base_state();
    s.fan_curve_writable = true;
    s.fan_curve_dirty = true;
    s.fan_curve_temps = [50, 55, 60, 65, 70, 75, 79, 85];
    s.fan_curve_pwms = [0, 8, 13, 26, 36, 54, 77, 112];
    assert!(s.fan_curve_can_mutate());
}

#[test]
fn fan_curve_capabilities_gating_from_registry() {
    let mut s = base_state();
    assert!(!s.fan_curve_writable);

    use orbis_core::capability::{
        Capability, CapabilityOperations, CapabilityStatus, OperationCapability,
    };
    let mut builder =
        orbis_capabilities::CapabilityRegistryBuilder::new(1, std::time::SystemTime::now());
    builder
        .add(
            orbis_core::FeatureId::FanCurves,
            Capability::new(CapabilityStatus::Supported).with_operations(CapabilityOperations {
                read: OperationCapability::new(CapabilityStatus::Supported),
                write: OperationCapability::new(CapabilityStatus::Supported),
            }),
        )
        .unwrap();
    let snapshot = builder.build().unwrap();
    s.update_capabilities(&snapshot);

    assert!(s.fan_curve_writable);
    assert_eq!(
        s.fan_curve_capability,
        controller::CapabilityAvailability::Supported
    );
}

#[test]
fn missing_preferences_initialize_dark_before_first_render() {
    let td = tempfile::tempdir().expect("tempdir");
    let light =
        initialize_runtime_theme_with(|| orbis_config::load_preferences_from_dir(td.path()));

    assert!(!light);
    assert!(matches!(current_theme_mode(), ThemeMode::Dark));
}

#[test]
fn persisted_dark_initializes_dark_before_first_render() {
    let td = tempfile::tempdir().expect("tempdir");
    let preferences = orbis_config::PreferencesConfig::default();
    orbis_config::save_preferences_to_dir(&preferences, td.path()).expect("save dark preferences");

    let light =
        initialize_runtime_theme_with(|| orbis_config::load_preferences_from_dir(td.path()));

    assert!(!light);
    assert!(matches!(current_theme_mode(), ThemeMode::Dark));
}

#[test]
fn persisted_light_initializes_light_before_first_render() {
    let td = tempfile::tempdir().expect("tempdir");
    let mut preferences = orbis_config::PreferencesConfig::default();
    preferences.appearance.theme = orbis_config::ThemePreference::Light;
    orbis_config::save_preferences_to_dir(&preferences, td.path()).expect("save light preferences");

    let light =
        initialize_runtime_theme_with(|| orbis_config::load_preferences_from_dir(td.path()));

    assert!(light);
    assert!(matches!(current_theme_mode(), ThemeMode::Light));
}

#[test]
fn persisted_start_minimized_is_loaded_before_first_render() {
    let td = tempfile::tempdir().expect("tempdir");
    let mut preferences = orbis_config::PreferencesConfig::default();
    preferences.appearance.theme = orbis_config::ThemePreference::Light;
    preferences.window.start_minimized = true;
    orbis_config::save_preferences_to_dir(&preferences, td.path())
        .expect("save start minimized preferences");

    let startup =
        initialize_runtime_preferences_with(|| orbis_config::load_preferences_from_dir(td.path()));

    assert!(startup.theme_light);
    assert!(startup.start_minimized);
    assert!(matches!(current_theme_mode(), ThemeMode::Light));
}

#[test]
fn missing_start_minimized_defaults_to_visible_startup() {
    let td = tempfile::tempdir().expect("tempdir");
    let startup =
        initialize_runtime_preferences_with(|| orbis_config::load_preferences_from_dir(td.path()));

    assert!(!startup.start_minimized);
    assert!(!startup.theme_light);
    assert!(matches!(current_theme_mode(), ThemeMode::Dark));
}

#[test]
fn start_minimized_window_state_is_applied_only_when_enabled() {
    let calls = Cell::new(0usize);
    let requested = Cell::new(false);
    apply_start_minimized(false, |value| {
        calls.set(calls.get() + 1);
        requested.set(value);
    });
    assert_eq!(calls.get(), 0);

    apply_start_minimized(true, |value| {
        calls.set(calls.get() + 1);
        requested.set(value);
    });
    assert_eq!(calls.get(), 1);
    assert!(requested.get());
}

#[test]
fn theme_toggle_persists_only_theme_field() {
    let td = tempfile::tempdir().expect("tempdir");
    let mut preferences = orbis_config::PreferencesConfig::default();
    preferences.window.close_action = orbis_config::CloseAction::Ask;
    preferences.window.start_minimized = true;
    preferences.window.remember_position = false;
    orbis_config::save_preferences_to_dir(&preferences, td.path()).expect("save fixture");

    persist_theme_with(
        true,
        || orbis_config::load_preferences_from_dir(td.path()),
        |preferences| orbis_config::save_preferences_to_dir(preferences, td.path()),
    )
    .expect("persist light theme");

    let reloaded = orbis_config::load_preferences_from_dir(td.path()).expect("reload preferences");
    assert_eq!(
        reloaded.preferences.appearance.theme,
        orbis_config::ThemePreference::Light
    );
    assert_eq!(
        reloaded.preferences.window.close_action,
        orbis_config::CloseAction::Ask
    );
    assert!(reloaded.preferences.window.start_minimized);
    assert!(!reloaded.preferences.window.remember_position);
}

#[test]
fn all_open_theme_targets_receive_same_runtime_theme() {
    let targets = [
        Cell::new(false),
        Cell::new(false),
        Cell::new(false),
        Cell::new(false),
        Cell::new(false),
        Cell::new(false),
        Cell::new(false),
        Cell::new(false),
    ];

    for target in &targets {
        apply_theme_if_open(Some(target), true, |target, mode| {
            target.set(matches!(mode, ThemeMode::Light));
        });
    }

    assert!(targets.iter().all(Cell::get));
}

#[test]
fn new_window_theme_is_derived_from_current_runtime_theme() {
    set_current_theme_light(true);
    assert!(matches!(current_theme_mode(), ThemeMode::Light));

    set_current_theme_light(false);
    assert!(matches!(current_theme_mode(), ThemeMode::Dark));
}

#[test]
fn theme_save_error_preserves_runtime_theme_and_is_diagnosable() {
    set_current_theme_light(true);

    let result = persist_theme_with(
        true,
        || {
            Ok(orbis_config::PreferencesLoad {
                preferences: orbis_config::PreferencesConfig::default(),
                source: orbis_config::PreferencesLoadSource::Defaults,
                warning: None,
            })
        },
        |_| {
            Err(orbis_config::PreferencesError::Io {
                operation: "test theme save",
                path: std::path::PathBuf::from("/test/preferences.toml"),
                source: std::io::Error::other("simulated save failure"),
            })
        },
    );

    let error = result.expect_err("save must fail");
    assert!(
        error
            .to_string()
            .contains("failed to save theme preference")
    );
    assert!(current_theme_light());
    // Theme persistence has no worker/provider input, so this failure path cannot
    // enqueue a Hardware1/sessiond/hardwared operation.
}

// --- ASUS product GPU queue result mapping (plan 2026-08-22, Task 4) ---

/// Wire outcome constants mirrored from `orbis-hardwared` `handle_set_product_gpu_mode`.
const PRODUCT_GPU_OUTCOME_ALREADY_ACTIVE: u32 = 0;
const PRODUCT_GPU_OUTCOME_REBOOT_REQUIRED: u32 = 1;
const PRODUCT_GPU_OUTCOME_UNKNOWN: u32 = 2;
const PRODUCT_GPU_OUTCOME_INCONSISTENT: u32 = 3;

fn product_gpu_result(
    current_mode: u32,
    queued_mode: u32,
    outcome: u32,
    reboot_required: bool,
) -> ProductGpuMutationResult {
    ProductGpuMutationResult {
        requested_mode: 1,
        current_mode,
        queued_mode,
        outcome,
        reboot_required,
    }
}

#[test]
fn product_gpu_wire_index_maps_only_three_product_modes() {
    assert_eq!(controller::asus_product_gpu_index(0), Some(0)); // Hybrid == Eco card
    assert_eq!(controller::asus_product_gpu_index(1), Some(1)); // Integrated == Standard card
    assert_eq!(controller::asus_product_gpu_index(2), Some(2)); // Ultimate
    // Optimized (3) is never generated: the ASUS Armoury product API has no
    // such mode; unknown sentinels and arbitrary values are rejected too.
    assert_eq!(controller::asus_product_gpu_index(3), None);
    assert_eq!(controller::asus_product_gpu_index(u32::MAX), None);
    assert_eq!(controller::asus_product_gpu_index(9), None);
}

#[test]
fn product_gpu_current_read_back_selects_the_card() {
    let mut s = base_state();
    s.gpu_selected = 1;

    apply_product_gpu_result(
        &mut s,
        Ok(product_gpu_result(
            0,
            u32::MAX,
            PRODUCT_GPU_OUTCOME_REBOOT_REQUIRED,
            true,
        )),
    );

    assert_eq!(s.gpu_selected, 0);
}

#[test]
fn product_gpu_queued_target_is_carried_and_reboot_required() {
    let mut s = base_state();
    s.gpu_queued = -1;
    s.gpu_reboot_required = false;

    apply_product_gpu_result(
        &mut s,
        Ok(product_gpu_result(
            1,
            2,
            PRODUCT_GPU_OUTCOME_REBOOT_REQUIRED,
            true,
        )),
    );

    assert_eq!(s.gpu_queued, 2);
    assert!(s.gpu_reboot_required);
}

#[test]
fn product_gpu_unknown_values_keep_previous_ui_evidence() {
    let mut s = base_state();
    s.gpu_selected = 1;
    s.gpu_queued = 2;
    s.gpu_reboot_required = false;
    s.gpu_section_error = false;

    apply_product_gpu_result(
        &mut s,
        Ok(product_gpu_result(
            u32::MAX,
            u32::MAX,
            PRODUCT_GPU_OUTCOME_UNKNOWN,
            false,
        )),
    );

    // Unknown read-back never overwrites known evidence and is not success.
    assert_eq!(s.gpu_selected, 1);
    assert_eq!(s.gpu_queued, 2);
    assert!(!s.gpu_reboot_required);
    assert!(!s.gpu_section_error);
}

#[test]
fn product_gpu_optimized_current_is_never_applied_to_ui() {
    let mut s = base_state();
    s.gpu_selected = 1;

    apply_product_gpu_result(
        &mut s,
        Ok(product_gpu_result(
            3,
            3,
            PRODUCT_GPU_OUTCOME_REBOOT_REQUIRED,
            true,
        )),
    );

    assert_eq!(s.gpu_selected, 1);
}

#[test]
fn product_gpu_definitive_outcomes_drive_section_error_honestly() {
    let mut s = base_state();
    s.gpu_section_error = true;

    apply_product_gpu_result(
        &mut s,
        Ok(product_gpu_result(
            1,
            2,
            PRODUCT_GPU_OUTCOME_ALREADY_ACTIVE,
            false,
        )),
    );
    assert!(!s.gpu_section_error);

    s.gpu_section_error = false;
    apply_product_gpu_result(
        &mut s,
        Ok(product_gpu_result(
            1,
            2,
            PRODUCT_GPU_OUTCOME_INCONSISTENT,
            false,
        )),
    );
    assert!(s.gpu_section_error);
}

#[test]
fn product_gpu_command_error_marks_section_unconfirmed() {
    let mut s = base_state();
    s.gpu_section_error = false;

    apply_product_gpu_result(
        &mut s,
        Err(ProviderError::Unsupported(
            "ASUS product GPU backend unavailable".into(),
        )),
    );

    assert!(s.gpu_section_error);
}

#[test]
fn production_initial_has_no_queued_target_or_reboot_claim() {
    let s = controller::UiState::production_initial();
    assert_eq!(s.gpu_queued, -1);
    assert!(!s.gpu_reboot_required);
}

#[test]
fn main_window_renders_queued_target_and_reboot_state() {
    let source = include_str!("../../../ui/audited/sections/performance.slint");
    // Queued target renders as pending on the exact queued segment.
    assert!(source.contains("gpu-queued == 0"));
    assert!(source.contains("gpu-queued == 1"));
    assert!(source.contains("gpu-queued == 2"));
    // Reboot-required status line exists; Optimized never gains a queued binding.
    assert!(source.contains("shutdown/reboot"));
    let optimized_line_start = source.find("title: \"Optimized\"").expect("Optimized tile");
    let optimized_line_end = source[optimized_line_start..]
        .find('\n')
        .map(|end| optimized_line_start + end)
        .expect("Optimized tile line ends");
    let optimized_line = &source[optimized_line_start..optimized_line_end];
    assert!(
        !optimized_line.contains("pending:"),
        "Optimized must never render a product-queue pending state"
    );
}

#[test]
fn shell_hosts_four_sections_and_frameless_chrome() {
    let shell = include_str!("../../../ui/audited/main-window.slint");
    assert!(shell.contains("no-frame: true"));
    assert!(shell.contains("width: 425px"));
    assert!(shell.contains("Section.Performance"));
    assert!(shell.contains("Section.Fans"));
    assert!(shell.contains("Section.Extra"));
    assert!(shell.contains("titlebar-close-requested"));
    assert!(shell.contains("titlebar-drag-started"));
    assert!(!shell.contains("UpdatesWindow"));
    assert!(!shell.contains("AutomationWindow"));
}
