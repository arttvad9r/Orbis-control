//! Private P2P D-Bus integration tests для profile-specific fan curve read.
//!
//! Проверяют совместимость между server method
//! `SessionService::fan_curve` (wire tuple `(uyayay)`) и client
//! `orbis_session_protocol::Session1Proxy::fan_curve` → `FanCurveInfo`.
//!
//! Используется пара локальных Unix streams (`std::os::unix::net::UnixStream::pair`);
//! внешний D-Bus daemon / system / session bus не задействованы. Sentinel-кривые
//! (B Balanced, C Quiet) проверяются на клиентской стороне через полный путь
//! Session1 → sessiond → scripted asusd source.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use orbis_core::action::ApplyResult;
use orbis_core::battery::{ChargeLimit, ChargeLimitBounds};
use orbis_core::diagnostics::DiagnosticEntry;
use orbis_core::fan::FanId;
use orbis_core::identity::BackendIdentity;
use orbis_core::newtypes::{FanPwm, Percent, TemperatureC};
use orbis_core::profile::AsusdFanProfile;
use orbis_providers::error::{ProviderError, ValidationResult};
use orbis_providers::traits::{BatteryProvider, Provider, ProviderHealth};
use orbis_session_protocol::{
    BUS_NAME, FanCurveInfo, OBJECT_PATH, Session1Proxy, fan_id, fan_profile,
};
use orbis_sessiond::fans::{AsusdFanCurve, AsusdFanCurveSet, AsusdFanCurveSource};
use orbis_sessiond::service::SessionService;
use zbus::connection::Builder;
use zbus::proxy::CacheProperties;

/// Минимальный scripted BatteryProvider (требуется конструктором SessionService).
struct ScriptedBatteryProvider;

#[async_trait]
impl BatteryProvider for ScriptedBatteryProvider {
    async fn charge_limit(&self) -> Result<ChargeLimit, ProviderError> {
        limit(true, None, 40, 100, 5)
    }
    async fn set_charge_limit(&self, _percent: u8) -> Result<ApplyResult, ProviderError> {
        Err(ProviderError::Unsupported("read-only".into()))
    }
    async fn one_shot_full_charge(&self) -> Result<ApplyResult, ProviderError> {
        Err(ProviderError::Unsupported("read-only".into()))
    }
    fn validate_charge_limit(&self, _percent: u8) -> ValidationResult {
        ValidationResult::invalid("read-only")
    }
}

impl Provider for ScriptedBatteryProvider {
    fn id(&self) -> &'static str {
        "scripted-battery"
    }
    fn backend(&self) -> BackendIdentity {
        BackendIdentity::simple("scripted-battery")
    }
    fn timeout(&self) -> Duration {
        Duration::from_millis(1)
    }
    fn explain_unsupported(&self, feature: &str) -> String {
        format!("scripted-battery: {feature} недоступен")
    }
    fn health(&self) -> ProviderHealth {
        ProviderHealth::Healthy
    }
    fn diagnostics(&self) -> Vec<DiagnosticEntry> {
        Vec::new()
    }
}

fn limit(
    enabled: bool,
    percent: Option<u8>,
    min: u8,
    max: u8,
    step: u8,
) -> Result<ChargeLimit, ProviderError> {
    ChargeLimit::new(
        enabled,
        percent.map(|p| Percent::new(p).expect("range")),
        percent.map(|p| Percent::new(p).expect("range")),
        Some(
            ChargeLimitBounds::new(
                Percent::new(min).expect("range"),
                Percent::new(max).expect("range"),
                step,
            )
            .expect("valid"),
        ),
    )
    .map_err(|e| ProviderError::Internal(e.to_string()))
}

/// Scripted asusd fan curve source с явными sentinel-кривыми per profile.
struct SentinelSource {
    cpu: HashMap<AsusdFanProfile, [u8; 8]>,
    gpu: HashMap<AsusdFanProfile, [u8; 8]>,
    pwm: HashMap<AsusdFanProfile, [u8; 8]>,
    reads: AtomicUsize,
}

fn curve(_profile: AsusdFanProfile, fan: FanId, temps: &[u8; 8], pwm: &[u8; 8]) -> AsusdFanCurve {
    let mut t = [TemperatureC::new(0).expect("const"); 8];
    let mut p = [FanPwm::new(0).expect("const"); 8];
    for i in 0..8 {
        t[i] = TemperatureC::new(i16::from(temps[i])).expect("temp");
        p[i] = FanPwm::new(pwm[i]).expect("pwm");
    }
    AsusdFanCurve {
        fan,
        temps: t,
        pwms: p,
        enabled: true,
    }
}

fn sentinel_source() -> SentinelSource {
    SentinelSource {
        cpu: HashMap::from([
            (AsusdFanProfile::Balanced, [40, 44, 50, 60, 70, 76, 82, 90]),
            (AsusdFanProfile::Quiet, [42, 46, 55, 64, 73, 80, 86, 92]),
        ]),
        gpu: HashMap::from([
            (AsusdFanProfile::Balanced, [25, 28, 31, 35, 40, 45, 50, 55]),
            (AsusdFanProfile::Quiet, [22, 25, 28, 32, 37, 42, 47, 52]),
        ]),
        pwm: HashMap::from([
            (AsusdFanProfile::Balanced, [3, 18, 35, 42, 50, 58, 70, 99]),
            (AsusdFanProfile::Quiet, [2, 12, 28, 38, 46, 54, 66, 88]),
        ]),
        reads: AtomicUsize::new(0),
    }
}

#[async_trait]
impl AsusdFanCurveSource for SentinelSource {
    async fn read_curves(
        &self,
        profile: AsusdFanProfile,
    ) -> Result<AsusdFanCurveSet, ProviderError> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        let cpu = self
            .cpu
            .get(&profile)
            .map(|temps| {
                curve(
                    profile,
                    FanId::Cpu,
                    temps,
                    self.pwm.get(&profile).expect("pwm"),
                )
            })
            .ok_or_else(|| ProviderError::Unsupported("cpu отсутствует".into()))?;
        let gpu = self
            .gpu
            .get(&profile)
            .map(|temps| {
                curve(
                    profile,
                    FanId::Gpu,
                    temps,
                    self.pwm.get(&profile).expect("pwm"),
                )
            })
            .ok_or_else(|| ProviderError::Unsupported("gpu отсутствует".into()))?;
        Ok(AsusdFanCurveSet { profile, cpu, gpu })
    }
}

/// Поднять пару P2P соединений поверх SessionService c fan source.
async fn connect_service(
    service: SessionService,
) -> Result<(zbus::Connection, zbus::Connection), Box<dyn std::error::Error + Send + Sync>> {
    let (server_stream, client_stream) = std::os::unix::net::UnixStream::pair()?;
    let guid = zbus::Guid::generate();
    let server_builder = Builder::unix_stream(server_stream)
        .server(guid)?
        .p2p()
        .name(BUS_NAME)?
        .serve_at(OBJECT_PATH, service)?;
    let client_builder = Builder::unix_stream(client_stream).p2p();
    let (server_conn, client_conn) =
        tokio::try_join!(server_builder.build(), client_builder.build())?;
    Ok((server_conn, client_conn))
}

async fn proxy_for(client: &zbus::Connection) -> zbus::Result<Session1Proxy<'_>> {
    Session1Proxy::builder(client)
        .cache_properties(CacheProperties::No)
        .build()
        .await
}

#[tokio::test]
async fn session1_fan_curve_reads_profile_specific_sentinel_over_p2p() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let source = Arc::new(sentinel_source());
        let service =
            SessionService::new(Arc::new(ScriptedBatteryProvider)).with_fan_curves(source.clone());
        let (_server, client) = connect_service(service).await.expect("p2p connect");
        let proxy = proxy_for(&client).await.expect("proxy");

        // RefreshFanCurve(Balanced, CPU) → кривая B (sentinel, не активная).
        let balanced: FanCurveInfo = proxy
            .fan_curve(fan_profile::BALANCED, fan_id::CPU)
            .await
            .expect("balanced read");
        assert_eq!(balanced.profile, fan_profile::BALANCED);
        assert_eq!(balanced.fan, fan_id::CPU);
        assert_eq!(balanced.temps.len(), 8);
        assert_eq!(balanced.temps[0], 40);
        assert_eq!(balanced.pwms[7], 99);

        // RefreshFanCurve(Quiet, CPU) → кривая C.
        let quiet: FanCurveInfo = proxy
            .fan_curve(fan_profile::QUIET, fan_id::CPU)
            .await
            .expect("quiet read");
        assert_eq!(quiet.profile, fan_profile::QUIET);
        assert_eq!(quiet.fan, fan_id::CPU);
        assert_eq!(quiet.temps[0], 42);
        assert_eq!(quiet.pwms[0], 2);

        assert_eq!(source.reads.load(Ordering::SeqCst), 2);
    })
    .await
    .expect("p2p test timeout");
}

#[tokio::test]
async fn session1_fan_curve_cpu_gpu_not_mixed() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let source = Arc::new(sentinel_source());
        let service =
            SessionService::new(Arc::new(ScriptedBatteryProvider)).with_fan_curves(source.clone());
        let (_server, client) = connect_service(service).await.expect("p2p connect");
        let proxy = proxy_for(&client).await.expect("proxy");

        let cpu: FanCurveInfo = proxy
            .fan_curve(fan_profile::BALANCED, fan_id::CPU)
            .await
            .expect("cpu");
        let gpu: FanCurveInfo = proxy
            .fan_curve(fan_profile::BALANCED, fan_id::GPU)
            .await
            .expect("gpu");

        assert_eq!(cpu.fan, fan_id::CPU);
        assert_eq!(gpu.fan, fan_id::GPU);
        assert_eq!(cpu.temps[0], 40);
        assert_eq!(gpu.temps[0], 25);
    })
    .await
    .expect("p2p test timeout");
}

#[tokio::test]
async fn session1_fan_curve_error_class_is_preserved() {
    tokio::time::timeout(Duration::from_secs(5), async {
        // Источник отсутствует: метод честно возвращает NotSupported.
        let service = SessionService::new(Arc::new(ScriptedBatteryProvider));
        let (_server, client) = connect_service(service).await.expect("p2p connect");
        let proxy = proxy_for(&client).await.expect("proxy");

        let err = proxy.fan_curve(0, 0).await.expect_err("unsupported");
        // zbus proxy для method call возвращает MethodError с remote fdo name.
        match err {
            zbus::Error::MethodError(name, msg, _) => {
                assert_eq!(
                    name.as_str(),
                    "org.freedesktop.DBus.Error.NotSupported",
                    "неожиданный error name: {name} - {msg:?}"
                );
            }
            zbus::Error::FDO(boxed) => {
                assert!(matches!(&*boxed, zbus::fdo::Error::NotSupported(_)));
            }
            other => panic!("ожидался NotSupported, получено: {other:?}"),
        }
    })
    .await
    .expect("p2p test timeout");
}
