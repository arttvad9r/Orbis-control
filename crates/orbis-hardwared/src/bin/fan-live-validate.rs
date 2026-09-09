//! Dev-only live-validation harness for the dormant asusd fan mutation
//! backend (#104/#105). NOT part of any production composition.
//!
//! Built only with `--features live-validation`; never shipped in release.
//!
//! What it does, in order (read-before-write everywhere):
//! 1. read-only `mutation_status()` evidence;
//! 2. observe kernel `/sys/firmware/acpi/platform_profile`;
//! 3. factory-defaults reset for one lossless asusd profile (#105 path) and
//!    verify a fresh FanCurveData observation plus an UNCHANGED platform
//!    profile;
//! 4. same-value custom write for the CPU fan: the curve bytes written are
//!    exactly the freshly stored ones, so fan behavior cannot change; the
//!    backend must preserve authoritative stored `enabled` (#104);
//! 5. independent re-read: CPU points AND enabled unchanged, GPU tuple byte-
//!    identical to its pre-write value (single-fan setter containment).
//!
//! The harness performs no sysfs writes itself. If the platform profile moves
//! during the reset it reports FAIL loudly and leaves restoration to the
//! operator (it deliberately has no platform_profile write surface).

use orbis_core::fan::FanId;
use orbis_core::newtypes::{FanPwm, TemperatureC};
use orbis_core::profile::AsusdFanProfile;
use orbis_hardwared::fans::{
    AsusdFanCurveClient, AsusdFanCurveMutationBackend, CURVE_POINT_COUNT,
    FanCurveMutationOperation, FanCurvePoints, ZbusAsusdFanCurveClient,
};

const PROFILE_PATH: &str = "/sys/firmware/acpi/platform_profile";

fn kernel_profile() -> std::io::Result<String> {
    Ok(std::fs::read_to_string(PROFILE_PATH)?.trim().to_owned())
}

type Wire = (String, [u8; 8], [u8; 8], bool);

fn find<'a>(raw: &'a [Wire], name: &str) -> Option<&'a Wire> {
    raw.iter().find(|(n, _, _, _)| n == name)
}

fn curve_from_wire(wire: &Wire) -> Result<(FanId, FanCurvePoints), String> {
    let fan = match wire.0.as_str() {
        "CPU" => FanId::Cpu,
        "GPU" => FanId::Gpu,
        other => return Err(format!("unsupported live-restore fan '{other}'")),
    };
    let mut temps = [TemperatureC::new(0).expect("0"); CURVE_POINT_COUNT];
    let mut pwms = [FanPwm::new(0).expect("0"); CURVE_POINT_COUNT];
    for i in 0..CURVE_POINT_COUNT {
        temps[i] = TemperatureC::new(wire.2[i] as i16)
            .map_err(|e| format!("{fan:?} temp {}: {e}", wire.2[i]))?;
        pwms[i] = FanPwm::new(wire.1[i]).map_err(|e| format!("{fan:?} pwm {}: {e}", wire.1[i]))?;
    }
    Ok((fan, FanCurvePoints { temps, pwms }))
}

async fn restore_original(
    backend: &AsusdFanCurveMutationBackend<ZbusAsusdFanCurveClient>,
    profile: AsusdFanProfile,
    original: &[Wire],
) -> Result<(), String> {
    for name in ["CPU", "GPU"] {
        let wire =
            find(original, name).ok_or_else(|| format!("original FanCurveData has no {name}"))?;
        let (fan, curve) = curve_from_wire(wire)?;
        backend
            .set_fan_curve(profile, &fan, &curve)
            .await
            .map_err(|e| format!("restore {name} curve failed: {e}"))?;
    }
    Ok(())
}

async fn run(profile: AsusdFanProfile) -> Result<(), String> {
    let connection = zbus::Connection::system()
        .await
        .map_err(|e| format!("system bus connect: {e}"))?;
    let client = ZbusAsusdFanCurveClient::new(connection);
    let backend = AsusdFanCurveMutationBackend::new(client.clone());

    // 1. read-only status evidence.
    let status = backend.mutation_status().await;
    println!("1. mutation_status            = {status:?}");
    if !matches!(status, orbis_hardwared::fans::FanMutationStatus::Supported) {
        return Err(format!(
            "precondition failed: mutation_status = {status:?}; refusing to continue"
        ));
    }

    // 2. observe kernel profile before anything mutates.
    let profile_before = kernel_profile().map_err(|e| format!("read {PROFILE_PATH}: {e}"))?;
    println!("2. platform_profile (before)  = {profile_before}");
    let original_raw = client
        .read_curves(profile)
        .await
        .map_err(|e| format!("pre-reset FanCurveData read: {e}"))?;
    macro_rules! restore_fail {
        ($message:expr) => {{
            let restore = restore_original(&backend, profile, &original_raw).await;
            return Err(format!("{}; restore={restore:?}", $message));
        }};
    }

    // 3. factory defaults for the whole profile (#105 upstream call), then
    //    fresh observation + platform-profile containment check.
    let defaults = match backend.reset_curves_to_defaults(profile).await {
        Ok(defaults) => defaults,
        Err(error) => {
            let restore = restore_original(&backend, profile, &original_raw).await;
            return Err(format!(
                "reset_curves_to_defaults({profile:?}) failed: {error}; restore={restore:?}"
            ));
        }
    };
    println!(
        "3. reset_curves_to_defaults   = {:?} observed_curves={}",
        defaults.result, defaults.observed_curves
    );
    let after_reset_raw = match client.read_curves(profile).await {
        Ok(raw) => raw,
        Err(error) => restore_fail!(format!("post-reset FanCurveData read: {error}")),
    };
    let profile_after_reset = match kernel_profile() {
        Ok(profile) => profile,
        Err(error) => restore_fail!(format!("re-read {PROFILE_PATH}: {error}")),
    };
    println!("   platform_profile (after)   = {profile_after_reset}");
    if profile_after_reset != profile_before {
        restore_fail!(format!(
            "#105 FAIL: platform_profile changed during factory-defaults reset ({profile_before} → {profile_after_reset})"
        ));
    }

    // 4. same-value CPU write from the freshly stored wire data.
    let Some((cpu_name, cpu_pwms, cpu_temps, enabled_before)) =
        find(&after_reset_raw, "CPU").cloned()
    else {
        restore_fail!("post-reset FanCurveData has no CPU entry");
    };
    let _ = cpu_name;
    let (cpu_fan, same_curve) =
        match curve_from_wire(&(cpu_name.clone(), cpu_pwms, cpu_temps, enabled_before)) {
            Ok(curve) => curve,
            Err(error) => restore_fail!(error),
        };
    println!(
        "4. same-value CPU write       = {} pts, stored enabled={enabled_before}",
        CURVE_POINT_COUNT
    );

    let gpu_before = find(&after_reset_raw, "GPU").cloned();

    let readback = match backend.set_fan_curve(profile, &cpu_fan, &same_curve).await {
        Ok(readback) => readback,
        Err(error) => restore_fail!(format!("same-value set_fan_curve failed: {error}")),
    };
    println!("   backend readback           = {:?}", readback.result);

    // 5. independent re-read: CPU bytes+enabled preserved, GPU untouched.
    let final_raw = match client.read_curves(profile).await {
        Ok(raw) => raw,
        Err(error) => restore_fail!(format!("final FanCurveData read: {error}")),
    };
    let Some((_, f_pwms, f_temps, enabled_after)) = find(&final_raw, "CPU") else {
        restore_fail!("final FanCurveData has no CPU entry");
    };
    if *enabled_after != enabled_before {
        restore_fail!(format!(
            "#104 FAIL: enabled drifted {enabled_before} → {enabled_after}"
        ));
    }
    if f_temps != &cpu_temps || f_pwms != &cpu_pwms {
        restore_fail!(format!(
            "#104 FAIL: CPU curve bytes drifted after same-value write (temps before={cpu_temps:?} after={f_temps:?}, pwms before={cpu_pwms:?} after={f_pwms:?})"
        ));
    }
    match (&gpu_before, find(&final_raw, "GPU")) {
        (Some(g0), Some(g1)) if g0 != g1 => {
            restore_fail!("#containment FAIL: GPU curve changed during CPU write");
        }
        _ => {}
    }
    restore_original(&backend, profile, &original_raw).await?;
    let restored_raw = client
        .read_curves(profile)
        .await
        .map_err(|e| format!("restored FanCurveData read: {e}"))?;
    if restored_raw != original_raw {
        return Err(
            "restore verification failed: FanCurveData differs from pre-test snapshot".into(),
        );
    }
    println!(
        "5. independent re-read        = CPU bytes equal, enabled={enabled_after} preserved, GPU untouched"
    );
    println!("6. original curves restored  = byte-identical");
    Ok(())
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let profile = match args.get(1).map(String::as_str) {
        None => AsusdFanProfile::Quiet,
        Some("quiet") => AsusdFanProfile::Quiet,
        Some("balanced") => AsusdFanProfile::Balanced,
        Some("performance") => AsusdFanProfile::Performance,
        Some(other) => {
            eprintln!("usage: fan-live-validate [quiet|balanced|performance]; got {other}");
            std::process::exit(2);
        }
    };
    println!("fan-live-validate: profile={profile:?} (dev harness, not production)");
    match run(profile).await {
        Ok(()) => {
            println!("RESULT: PASS (#104 enabled preservation + #105 reset containment, live)");
        }
        Err(e) => {
            eprintln!("RESULT: FAIL: {e}");
            std::process::exit(1);
        }
    }
}
