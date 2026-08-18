use super::*;
use orbis_core::battery::{ChargeLimit, ChargeLimitBounds};
use orbis_core::fan::FanId;
use orbis_core::gpu::{GpuAccessPolicy, GpuMuxState, GpuPowerState};
use orbis_core::newtypes::Percent;

fn base_state() -> controller::UiState {
    controller::UiState::from_mock_profile("zephyrus-full")
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
fn gpu_index_mapping() {
    assert_eq!(gpu_mode_from_index(0), Some(GpuMode::Eco));
    assert_eq!(gpu_mode_from_index(1), Some(GpuMode::Standard));
    assert_eq!(gpu_mode_from_index(2), Some(GpuMode::Ultimate));
    assert_eq!(gpu_mode_from_index(3), Some(GpuMode::Optimized));
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
    let missing_effective = ChargeLimit::new(
        false,
        Some(Percent::new(80).unwrap()),
        None,
        None,
    )
    .unwrap();
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
            fan: orbis_core::fan::FanId::Cpu,
            rpm: orbis_core::newtypes::Rpm::new(2600).unwrap(),
            percent: None,
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

fn perf_state(
    current: PerformanceProfile,
    available: &[PerformanceProfile],
) -> PerformanceState {
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
fn main_window_height_no_longer_reserves_fan_editor_space() {
    let s = base_state();
    assert_eq!(window_height(&s), 441.0);

    let mut error = s;
    error.gpu_section_error = true;
    assert_eq!(window_height(&error), 466.0);
}
