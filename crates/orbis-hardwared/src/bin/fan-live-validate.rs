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

    // 3. factory defaults for the whole profile (#105 upstream call), then
    //    fresh observation + platform-profile containment check.
    let defaults = backend.reset_curves_to_defaults(profile).await.map_err(|e| {
        format!("reset_curves_to_defaults({profile:?}) failed: {e} — platform_profile may need manual restore if asusd switched it")
    })?;
    println!(
        "3. reset_curves_to_defaults   = {:?} observed_curves={}",
        defaults.result, defaults.observed_curves
    );
    let after_reset_raw = client
        .read_curves(profile)
        .await
        .map_err(|e| format!("post-reset FanCurveData read: {e}"))?;
    let profile_after_reset =
        kernel_profile().map_err(|e| format!("re-read {PROFILE_PATH}: {e}"))?;
    println!("   platform_profile (after)   = {profile_after_reset}");
    if profile_after_reset != profile_before {
        return Err(format!(
            "#105 FAIL: platform_profile changed during factory-defaults reset ({profile_before} → {profile_after_reset}); restore it manually"
        ));
    }

    // 4. same-value CPU write from the freshly stored wire data.
    let Some((cpu_name, cpu_pwms, cpu_temps, enabled_before)) =
        find(&after_reset_raw, "CPU").cloned()
    else {
        return Err("post-reset FanCurveData has no CPU entry".into());
    };
    let _ = cpu_name;
    let mut temps = [TemperatureC::new(0).expect("0"); CURVE_POINT_COUNT];
    let mut pwms = [FanPwm::new(0).expect("0"); CURVE_POINT_COUNT];
    for i in 0..CURVE_POINT_COUNT {
        temps[i] = TemperatureC::new(cpu_temps[i] as i16)
            .map_err(|e| format!("temp {}: {e}", cpu_temps[i]))?;
        pwms[i] = FanPwm::new(cpu_pwms[i]).map_err(|e| format!("pwm {}: {e}", cpu_pwms[i]))?;
    }
    let same_curve = FanCurvePoints { temps, pwms };
    println!(
        "4. same-value CPU write       = {} pts, stored enabled={enabled_before}",
        CURVE_POINT_COUNT
    );

    let gpu_before = find(&after_reset_raw, "GPU").cloned();

    let readback = backend
        .set_fan_curve(profile, &FanId::Cpu, &same_curve)
        .await
        .map_err(|e| format!("same-value set_fan_curve failed: {e}"))?;
    println!("   backend readback           = {:?}", readback.result);

    // 5. independent re-read: CPU bytes+enabled preserved, GPU untouched.
    let final_raw = client
        .read_curves(profile)
        .await
        .map_err(|e| format!("final FanCurveData read: {e}"))?;
    let Some((_, f_temps, f_pwms, enabled_after)) = find(&final_raw, "CPU") else {
        return Err("final FanCurveData has no CPU entry".into());
    };
    if *enabled_after != enabled_before {
        return Err(format!(
            "#104 FAIL: enabled drifted {enabled_before} → {enabled_after}"
        ));
    }
    if f_temps != &cpu_temps || f_pwms != &cpu_pwms {
        return Err("#104 FAIL: CPU curve bytes drifted after same-value write".into());
    }
    match (&gpu_before, find(&final_raw, "GPU")) {
        (Some(g0), Some(g1)) if g0 != g1 => {
            return Err("#containment FAIL: GPU curve changed during CPU write".into());
        }
        _ => {}
    }
    println!(
        "5. independent re-read        = CPU bytes equal, enabled={enabled_after} preserved, GPU untouched"
    );
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
