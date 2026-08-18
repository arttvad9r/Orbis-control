//! Read-only Wayland display output provider (session concern).
//!
//! Это НЕ ASUS firmware capability и НЕ hardwared. Это compositor (Wayland)
//! session concern: провайдер читает, какие outputs compositor реально
//! показывает и какой current mode/refresh он сообщает.
//!
//! Архитектура разделена на два слоя:
//!
//! 1. **Чистый testable aggregator** ([`WaylandOutputAggregator`]) — детерминированная
//!    state machine над типизированными событиями ([`OutputEvent`]), зеркалящими
//!    `wl_output` protocol. Не зависит от Wayland и полностью тестируется
//!    scripted-событиями.
//! 2. **Тонкий production adapter** ([`WaylandDisplayOutputProvider`]) — подключается
//!    к compositor через `wayland-client`, биндит `wl_output` globals и
//!    диспатчит события в aggregator. Никакой логики агрегации здесь нет.
//!
//! Контракт строго по core `wl_output` (wayland.xml):
//! - `mode`: `flags` (current=0x1, preferred=0x2), width/height (hardware units),
//!   refresh (mHz);
//! - «there will always be one mode, the current mode»; «the current mode is
//!   always the last mode that was received with the current flag set»;
//! - non-current modes deprecated; compositor может слать только current;
//! - refresh 0 = «не имеет смысла для этого output»;
//! - `name` (v4+): runtime identity, не persistent, не обязан быть DRM connector;
//! - `done` (v2+): атомарная граница набора событий.
//!
//! Провайдер read-only: никакого modeset/configuration API, никаких writes.

use std::collections::HashMap;
use std::time::Duration;

use async_trait::async_trait;
use orbis_core::diagnostics::DiagnosticEntry;
use orbis_core::display_output::{
    CurrentDisplayMode, DisplayMode, DisplayOutputId, DisplayOutputSnapshot, DisplayOutputState,
};
use orbis_core::identity::BackendIdentity;
use orbis_core::newtypes::RefreshMilliHz;

use crate::error::ProviderError;
use crate::traits::{DisplayOutputProvider, Provider, ProviderHealth};

/// Типизированное событие `wl_output`, зеркалящее protocol.
///
/// Используется aggregator-ом; production adapter конвертирует
/// `wl_output::Event` в эти события. Отдельный тип позволяет тестировать
/// state machine без Wayland.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputEvent {
    /// `wl_output.mode`: flags + width/height/refresh (mHz).
    Mode {
        /// Флаг `current` (0x1).
        current: bool,
        /// Флаг `preferred` (0x2).
        preferred: bool,
        /// Ширина в hardware units.
        width: u32,
        /// Высота в hardware units.
        height: u32,
        /// Вертикальная частота в mHz.
        refresh: u32,
    },
    /// `wl_output.name` (v4+): runtime identity.
    Name(String),
    /// `wl_output.done` (v2+): атомарная граница набора событий.
    Done,
}

/// Внутреннее состояние одного output-а в aggregator-е.
#[derive(Debug, Clone, PartialEq, Eq)]
struct OutputAccumulator {
    /// Runtime identity (из `name`), если compositor его прислал.
    name: Option<String>,
    /// Текущий mode (последний полученный с флагом `current`).
    current: Option<DisplayMode>,
    /// Preferred mode, если compositor его прислал.
    preferred: Option<DisplayMode>,
    /// Наблюдение non-current modes (optional, не обязательная часть контракта).
    available: Vec<DisplayMode>,
}

impl OutputAccumulator {
    fn new() -> Self {
        Self {
            name: None,
            current: None,
            preferred: None,
            available: Vec::new(),
        }
    }

    fn apply(&mut self, event: &OutputEvent) {
        match event {
            OutputEvent::Mode {
                current,
                preferred,
                width,
                height,
                refresh,
            } => {
                let mode =
                    DisplayMode::new(*width, *height, RefreshMilliHz::new(*refresh).unwrap());
                if *current {
                    // «the current mode is always the last mode that was received
                    // with the current flag set» — заменяем, не накапливаем.
                    self.current = Some(mode);
                }
                if *preferred {
                    self.preferred = Some(mode);
                }
                // Non-current modes — optional observation; сохраняем только
                // если compositor их прислал. Отсутствие — не ошибка.
                if !*current {
                    self.available.push(mode);
                }
            }
            OutputEvent::Name(name) => {
                self.name = Some(name.clone());
            }
            OutputEvent::Done => {}
        }
    }

    /// Завершить output в snapshot state.
    ///
    /// Если current mode отсутствует (compositor не прислал ни одного mode с
    /// флагом `current`) — output не публикуется: protocol гарантирует, что
    /// current mode всегда есть, поэтому его отсутствие — противоречивое
    /// evidence, а не валидное состояние.
    fn finish(&self) -> Option<DisplayOutputState> {
        let current = self.current?;
        let id = DisplayOutputId::new(self.name.clone().unwrap_or_else(|| "<unnamed>".to_string()));
        Some(DisplayOutputState {
            id,
            current_mode: CurrentDisplayMode {
                current,
                preferred: self.preferred,
            },
            available_modes: self.available.clone(),
        })
    }
}

/// Чистый testable aggregator событий `wl_output`.
///
/// Владеет состоянием outputs и собирает snapshot. Не выполняет I/O и не
/// зависит от Wayland. Каждый output идентифицируется по compositor-provided
/// name (runtime identity); если name отсутствует — используется placeholder,
/// но output всё равно представляется (name optional по protocol).
#[derive(Debug, Default)]
pub struct WaylandOutputAggregator {
    outputs: HashMap<String, OutputAccumulator>,
}

impl WaylandOutputAggregator {
    /// Создать пустой aggregator.
    pub fn new() -> Self {
        Self::default()
    }

    /// Применить событие к output с данным runtime identity.
    ///
    /// `output_key` — runtime identity output-а (compositor-provided name или
    /// placeholder для безымянного output-а).
    pub fn apply(&mut self, output_key: &str, event: &OutputEvent) {
        self.outputs
            .entry(output_key.to_string())
            .or_insert_with(OutputAccumulator::new)
            .apply(event);
    }

    /// Удалить output (hotplug/removal).
    pub fn remove_output(&mut self, output_key: &str) {
        self.outputs.remove(output_key);
    }

    /// Собрать snapshot текущих outputs.
    ///
    /// Outputs без current mode (противоречивое evidence) пропускаются.
    pub fn snapshot(&self) -> DisplayOutputSnapshot {
        let mut outputs: Vec<DisplayOutputState> = self
            .outputs
            .iter()
            .filter_map(|(key, acc)| {
                let mut state = acc.finish()?;
                // Если name не был предоставлен через Name event, используем key как ID
                if state.id.as_str() == "<unnamed>" {
                    state.id = DisplayOutputId::new(key.clone());
                }
                Some(state)
            })
            .collect();
        // Детерминированный порядок по runtime identity.
        outputs.sort_by(|a, b| a.id.as_str().cmp(b.id.as_str()));
        DisplayOutputSnapshot { outputs }
    }
}

/// Ошибка подключения к Wayland compositor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WaylandConnectError {
    /// Нет Wayland session/display (нет `WAYLAND_DISPLAY`/socket).
    NoDisplay,
    /// Подключение к compositor не удалось.
    ConnectFailed(String),
}

/// Testable источник Wayland output snapshot.
#[async_trait]
pub trait WaylandOutputSource: Send + Sync {
    /// Прочитать authoritative snapshot outputs compositor-а.
    async fn read_snapshot(&self) -> Result<DisplayOutputSnapshot, ProviderError>;
}

/// Read-only provider над Wayland compositor.
///
/// `S` — источник (реальный Wayland adapter или scripted в тестах).
pub struct WaylandDisplayOutputProvider<S> {
    source: S,
}

impl<S> WaylandDisplayOutputProvider<S> {
    /// Создать provider над source.
    ///
    /// Не подключается к compositor и не выполняет I/O.
    pub fn new(source: S) -> Self {
        Self { source }
    }
}

impl<S> Provider for WaylandDisplayOutputProvider<S>
where
    S: WaylandOutputSource,
{
    fn id(&self) -> &'static str {
        "wayland-display-output"
    }

    fn backend(&self) -> BackendIdentity {
        BackendIdentity::simple("wayland")
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(1)
    }

    fn explain_unsupported(&self, feature: &str) -> String {
        format!("wayland read-only backend: функция '{feature}' недоступна")
    }

    fn health(&self) -> ProviderHealth {
        ProviderHealth::Healthy
    }

    fn diagnostics(&self) -> Vec<DiagnosticEntry> {
        vec![DiagnosticEntry::new(
            "provider.wayland-display-output",
            "read-only Wayland compositor display output backend",
        )]
    }
}

#[async_trait]
impl<S> DisplayOutputProvider for WaylandDisplayOutputProvider<S>
where
    S: WaylandOutputSource,
{
    async fn display_output_snapshot(&self) -> Result<DisplayOutputSnapshot, ProviderError> {
        self.source.read_snapshot().await
    }
}

// ---------------------------------------------------------------------------
// Production Wayland adapter (thin over the pure aggregator)
// ---------------------------------------------------------------------------

/// Состояние dispatch-а Wayland событий.
///
/// Хранит aggregator и runtime identity каждого output-а.
#[derive(Default)]
pub struct WaylandState {
    aggregator: WaylandOutputAggregator,
    /// output_key → получил ли done.
    done: std::collections::HashMap<String, bool>,
}

impl WaylandState {
    /// Создать пустое состояние.
    pub fn new() -> Self {
        Self::default()
    }

    /// Все ли известные outputs получили `done`.
    fn all_done(&self) -> bool {
        !self.done.is_empty() && self.done.values().all(|d| *d)
    }

    /// Собрать snapshot из агрегированного состояния.
    fn snapshot(&self) -> DisplayOutputSnapshot {
        self.aggregator.snapshot()
    }
}

impl wayland_client::Dispatch<wayland_client::protocol::wl_output::WlOutput, String>
    for WaylandState
{
    fn event(
        state: &mut Self,
        _proxy: &wayland_client::protocol::wl_output::WlOutput,
        event: wayland_client::protocol::wl_output::Event,
        data: &String,
        _conn: &wayland_client::Connection,
        _qhandle: &wayland_client::QueueHandle<Self>,
    ) {
        let key = data.clone();
        let output_event = match event {
            wayland_client::protocol::wl_output::Event::Mode {
                flags,
                width,
                height,
                refresh,
            } => {
                // flags: WEnum<Mode> — unwrap to get actual bitflags
                let flags_bits = match flags {
                    wayland_client::WEnum::Value(f) => f.bits(),
                    wayland_client::WEnum::Unknown(bits) => bits,
                };
                let current = flags_bits & 0x1 != 0; // Mode::Current = 0x1
                let preferred = flags_bits & 0x2 != 0; // Mode::Preferred = 0x2
                OutputEvent::Mode {
                    current,
                    preferred,
                    width: width as u32,
                    height: height as u32,
                    refresh: refresh as u32,
                }
            }
            wayland_client::protocol::wl_output::Event::Name { name } => OutputEvent::Name(name),
            wayland_client::protocol::wl_output::Event::Done => {
                state.done.insert(key.clone(), true);
                OutputEvent::Done
            }
            // Geometry/scale/description не влияют на current mode/refresh.
            _ => return,
        };
        state.aggregator.apply(&key, &output_event);
    }
}

impl wayland_client::Dispatch<wayland_client::protocol::wl_registry::WlRegistry, ()>
    for WaylandState
{
    fn event(
        _state: &mut Self,
        _proxy: &wayland_client::protocol::wl_registry::WlRegistry,
        _event: wayland_client::protocol::wl_registry::Event,
        _data: &(),
        _conn: &wayland_client::Connection,
        _qhandle: &wayland_client::QueueHandle<Self>,
    ) {
    }
}

impl
    wayland_client::Dispatch<
        wayland_client::protocol::wl_registry::WlRegistry,
        wayland_client::globals::GlobalListContents,
    > for WaylandState
{
    fn event(
        _state: &mut Self,
        _proxy: &wayland_client::protocol::wl_registry::WlRegistry,
        _event: wayland_client::protocol::wl_registry::Event,
        _data: &wayland_client::globals::GlobalListContents,
        _conn: &wayland_client::Connection,
        _qhandle: &wayland_client::QueueHandle<Self>,
    ) {
    }
}

/// Реальный Wayland источник: подключается к compositor, биндит `wl_output`
/// globals и собирает snapshot через [`WaylandOutputAggregator`].
///
/// Тонкий слой: вся логика агрегации — в aggregator-е; здесь только
/// подключение, bind и dispatch событий. Read-only: никакого modeset/
/// configuration API.
#[derive(Clone)]
pub struct WaylandCompositorOutputSource {
    /// Runtime identity placeholder для безымянных outputs.
    unnamed_prefix: String,
}

impl Default for WaylandCompositorOutputSource {
    fn default() -> Self {
        Self {
            unnamed_prefix: "output-".to_string(),
        }
    }
}

impl WaylandCompositorOutputSource {
    /// Создать источник.
    pub fn new() -> Self {
        Self::default()
    }

    /// Подключиться к compositor и собрать snapshot.
    ///
    /// - нет Wayland session/display → `ProviderError::BackendUnavailable`
    ///   (не Unsupported: это отсутствие backend, а не отсутствие capability);
    /// - подключение не удалось → `ProviderError::BackendUnavailable`;
    /// - `wl_output` global отсутствует → `ProviderError::Unsupported`
    ///   (compositor не предоставляет wl_output);
    /// - нет outputs → пустой snapshot (не fake output).
    pub fn read_snapshot_blocking(&self) -> Result<DisplayOutputSnapshot, ProviderError> {
        let conn = wayland_client::Connection::connect_to_env()
            .map_err(|e| ProviderError::BackendUnavailable(format!("wayland connect: {e}")))?;
        let (globals, mut queue) =
            wayland_client::globals::registry_queue_init::<WaylandState>(&conn).map_err(|e| {
                ProviderError::BackendUnavailable(format!("wayland registry init: {e}"))
            })?;

        let mut state = WaylandState::new();
        let mut unnamed_counter = 0usize;
        let mut bound_any = false;

        // Access the global list via the registry's data
        use wayland_client::Proxy;
        let global_list = globals
            .registry()
            .data::<wayland_client::globals::GlobalListContents>()
            .ok_or_else(|| ProviderError::Internal("wayland registry data отсутствует".into()))?;
        for global in global_list.clone_list() {
            if global.interface != "wl_output" {
                continue;
            }
            bound_any = true;
            let key = format!("{}{}", self.unnamed_prefix, unnamed_counter);
            unnamed_counter += 1;
            let output = globals
                .bind::<wayland_client::protocol::wl_output::WlOutput, _, _>(
                    &queue.handle(),
                    1..=4,
                    key.clone(),
                )
                .map_err(|e| {
                    ProviderError::BackendUnavailable(format!("wayland bind wl_output: {e}"))
                })?;
            // Сохраняем proxy, чтобы он не был уничтожен до dispatch.
            let _ = output;
        }

        if !bound_any {
            return Err(ProviderError::Unsupported(
                "compositor не предоставляет wl_output global".into(),
            ));
        }

        // Dispatch события до тех пор, пока все outputs не получат done
        // (или не исчерпаем roundtrip-лимит, чтобы не зависнуть).
        let max_roundtrips = 8;
        for _ in 0..max_roundtrips {
            let _ = queue.roundtrip(&mut state).map_err(|e| {
                ProviderError::BackendUnavailable(format!("wayland roundtrip: {e}"))
            })?;
            if state.all_done() {
                break;
            }
        }

        Ok(state.snapshot())
    }
}

#[async_trait]
impl WaylandOutputSource for WaylandCompositorOutputSource {
    async fn read_snapshot(&self) -> Result<DisplayOutputSnapshot, ProviderError> {
        // Wayland client API синхронный (roundtrip); выполняем в
        // spawn_blocking, чтобы не блокировать async runtime.
        let source = self.clone();
        tokio::task::spawn_blocking(move || source.read_snapshot_blocking())
            .await
            .map_err(|e| ProviderError::Internal(format!("wayland read task: {e}")))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mode(w: u32, h: u32, mhz: u32) -> DisplayMode {
        DisplayMode::new(w, h, RefreshMilliHz::new(mhz).unwrap())
    }

    fn current(w: u32, h: u32, mhz: u32) -> OutputEvent {
        OutputEvent::Mode {
            current: true,
            preferred: false,
            width: w,
            height: h,
            refresh: mhz,
        }
    }

    fn preferred(w: u32, h: u32, mhz: u32) -> OutputEvent {
        OutputEvent::Mode {
            current: false,
            preferred: true,
            width: w,
            height: h,
            refresh: mhz,
        }
    }

    fn non_current(w: u32, h: u32, mhz: u32) -> OutputEvent {
        OutputEvent::Mode {
            current: false,
            preferred: false,
            width: w,
            height: h,
            refresh: mhz,
        }
    }

    #[test]
    fn one_output_one_current_mode() {
        let mut agg = WaylandOutputAggregator::new();
        agg.apply("eDP-1", &OutputEvent::Name("eDP-1".into()));
        agg.apply("eDP-1", &current(1920, 1080, 60000));
        agg.apply("eDP-1", &OutputEvent::Done);
        let snap = agg.snapshot();
        assert_eq!(snap.outputs.len(), 1);
        let out = &snap.outputs[0];
        assert_eq!(out.id.as_str(), "eDP-1");
        assert_eq!(out.current_mode.current, mode(1920, 1080, 60000));
        assert!(out.current_mode.preferred.is_none());
        assert!(out.available_modes.is_empty());
    }

    #[test]
    fn exact_60000_mhz_preserved() {
        let mut agg = WaylandOutputAggregator::new();
        agg.apply("eDP-1", &current(1920, 1080, 60000));
        assert_eq!(
            agg.snapshot().outputs[0].current_mode.current.refresh.get(),
            60000
        );
    }

    #[test]
    fn fractional_59940_mhz_preserved_losslessly() {
        let mut agg = WaylandOutputAggregator::new();
        agg.apply("eDP-1", &current(1920, 1080, 59940));
        let refresh = agg.snapshot().outputs[0].current_mode.current.refresh.get();
        assert_eq!(refresh, 59940);
        assert_ne!(refresh, 60000);
    }

    #[test]
    fn one_hundred_twenty_thousand_mhz_preserved() {
        let mut agg = WaylandOutputAggregator::new();
        agg.apply("eDP-1", &current(2560, 1600, 120000));
        assert_eq!(
            agg.snapshot().outputs[0].current_mode.current.refresh.get(),
            120000
        );
    }

    #[test]
    fn refresh_zero_handled_as_not_meaningful() {
        // 0 допустим (virtual output), не становится 60 Hz.
        let mut agg = WaylandOutputAggregator::new();
        agg.apply("WL-1", &current(1920, 1080, 0));
        assert_eq!(
            agg.snapshot().outputs[0].current_mode.current.refresh.get(),
            0
        );
    }

    #[test]
    fn output_without_optional_name_still_represented() {
        // name optional по protocol; output без name всё равно представляется.
        let mut agg = WaylandOutputAggregator::new();
        agg.apply("out-1", &current(1920, 1080, 60000));
        let snap = agg.snapshot();
        assert_eq!(snap.outputs.len(), 1);
        assert_eq!(snap.outputs[0].id.as_str(), "out-1");
    }

    #[test]
    fn named_output_identity_preserved() {
        let mut agg = WaylandOutputAggregator::new();
        agg.apply("HDMI-A-1", &OutputEvent::Name("HDMI-A-1".into()));
        agg.apply("HDMI-A-1", &current(3840, 2160, 60000));
        assert_eq!(agg.snapshot().outputs[0].id.as_str(), "HDMI-A-1");
    }

    #[test]
    fn width_height_preserved() {
        let mut agg = WaylandOutputAggregator::new();
        agg.apply("eDP-1", &current(2560, 1600, 120000));
        let m = agg.snapshot().outputs[0].current_mode.current;
        assert_eq!(m.width, 2560);
        assert_eq!(m.height, 1600);
    }

    #[test]
    fn preferred_flag_does_not_imply_current() {
        // preferred mode не становится current автоматически.
        let mut agg = WaylandOutputAggregator::new();
        agg.apply("eDP-1", &current(1920, 1080, 60000));
        agg.apply("eDP-1", &preferred(1920, 1080, 120000));
        let out = &agg.snapshot().outputs[0];
        assert_eq!(out.current_mode.current.refresh.get(), 60000);
        assert_eq!(out.current_mode.preferred.unwrap().refresh.get(), 120000);
    }

    #[test]
    fn current_flag_determines_current_mode() {
        // Последний mode с флагом current — текущий.
        let mut agg = WaylandOutputAggregator::new();
        agg.apply("eDP-1", &current(1920, 1080, 60000));
        agg.apply("eDP-1", &current(1920, 1080, 120000));
        assert_eq!(
            agg.snapshot().outputs[0].current_mode.current.refresh.get(),
            120000
        );
    }

    #[test]
    fn non_current_modes_absent_is_valid() {
        // Compositor может слать только current mode; отсутствие non-current
        // не ошибка и не Unsupported.
        let mut agg = WaylandOutputAggregator::new();
        agg.apply("eDP-1", &current(1920, 1080, 60000));
        let out = &agg.snapshot().outputs[0];
        assert!(out.available_modes.is_empty());
    }

    #[test]
    fn non_current_modes_present_is_valid_observation() {
        let mut agg = WaylandOutputAggregator::new();
        agg.apply("eDP-1", &current(1920, 1080, 120000));
        agg.apply("eDP-1", &non_current(1920, 1080, 60000));
        let out = &agg.snapshot().outputs[0];
        assert_eq!(out.available_modes.len(), 1);
        assert_eq!(out.available_modes[0].refresh.get(), 60000);
    }

    #[test]
    fn multiple_outputs_isolated() {
        let mut agg = WaylandOutputAggregator::new();
        agg.apply("eDP-1", &current(2560, 1600, 120000));
        agg.apply("HDMI-A-1", &current(3840, 2160, 60000));
        let snap = agg.snapshot();
        assert_eq!(snap.outputs.len(), 2);
        assert_eq!(snap.outputs[0].id.as_str(), "HDMI-A-1");
        assert_eq!(snap.outputs[1].id.as_str(), "eDP-1");
    }

    #[test]
    fn one_output_changes_current_mode() {
        let mut agg = WaylandOutputAggregator::new();
        agg.apply("eDP-1", &current(1920, 1080, 60000));
        assert_eq!(
            agg.snapshot().outputs[0].current_mode.current.refresh.get(),
            60000
        );
        agg.apply("eDP-1", &current(1920, 1080, 120000));
        assert_eq!(
            agg.snapshot().outputs[0].current_mode.current.refresh.get(),
            120000
        );
    }

    #[test]
    fn old_current_state_correctly_replaced() {
        // Старый current mode не смешивается с новым batch.
        let mut agg = WaylandOutputAggregator::new();
        agg.apply("eDP-1", &current(1920, 1080, 60000));
        agg.apply("eDP-1", &current(1920, 1080, 120000));
        let out = &agg.snapshot().outputs[0];
        assert_eq!(out.current_mode.current.refresh.get(), 120000);
        // available_modes содержит только non-current наблюдения, не старый current.
        assert!(out.available_modes.is_empty());
    }

    #[test]
    fn output_removal_removes_only_that_output() {
        let mut agg = WaylandOutputAggregator::new();
        agg.apply("eDP-1", &current(2560, 1600, 120000));
        agg.apply("HDMI-A-1", &current(3840, 2160, 60000));
        agg.remove_output("eDP-1");
        let snap = agg.snapshot();
        assert_eq!(snap.outputs.len(), 1);
        assert_eq!(snap.outputs[0].id.as_str(), "HDMI-A-1");
    }

    #[test]
    fn hotplug_adds_output_without_corrupting_existing() {
        let mut agg = WaylandOutputAggregator::new();
        agg.apply("eDP-1", &current(2560, 1600, 120000));
        agg.apply("HDMI-A-1", &current(3840, 2160, 60000));
        let snap = agg.snapshot();
        assert_eq!(snap.outputs.len(), 2);
        assert_eq!(snap.outputs[0].id.as_str(), "HDMI-A-1");
        assert_eq!(snap.outputs[1].id.as_str(), "eDP-1");
    }

    #[test]
    fn done_boundary_produces_coherent_snapshot() {
        // done — атомарная граница; snapshot после done согласован.
        let mut agg = WaylandOutputAggregator::new();
        agg.apply("eDP-1", &OutputEvent::Name("eDP-1".into()));
        agg.apply("eDP-1", &current(1920, 1080, 60000));
        agg.apply("eDP-1", &OutputEvent::Done);
        let out = &agg.snapshot().outputs[0];
        assert_eq!(out.id.as_str(), "eDP-1");
        assert_eq!(out.current_mode.current.refresh.get(), 60000);
    }

    #[test]
    fn contradictory_multiple_current_modes_handled_deterministically() {
        // Protocol: current mode = последний mode с флагом current. Два current
        // события — последнее побеждает (детерминированно, fail-closed к
        // последнему authoritative событию).
        let mut agg = WaylandOutputAggregator::new();
        agg.apply("eDP-1", &current(1920, 1080, 60000));
        agg.apply("eDP-1", &current(1920, 1080, 120000));
        assert_eq!(
            agg.snapshot().outputs[0].current_mode.current.refresh.get(),
            120000
        );
    }

    #[test]
    fn no_outputs_does_not_create_fake_output() {
        let agg = WaylandOutputAggregator::new();
        let snap = agg.snapshot();
        assert!(snap.outputs.is_empty());
    }

    #[test]
    fn output_without_current_mode_is_not_published() {
        // Protocol гарантирует current mode всегда есть; его отсутствие —
        // противоречивое evidence, output не публикуется.
        let mut agg = WaylandOutputAggregator::new();
        agg.apply("eDP-1", &OutputEvent::Name("eDP-1".into()));
        agg.apply("eDP-1", &non_current(1920, 1080, 60000));
        assert!(agg.snapshot().outputs.is_empty());
    }

    #[test]
    fn generic_provider_does_not_require_available_mode_enumeration() {
        // Provider Supported, даже если compositor прислал только current mode.
        let mut agg = WaylandOutputAggregator::new();
        agg.apply("eDP-1", &current(1920, 1080, 60000));
        let snap = agg.snapshot();
        assert_eq!(snap.outputs.len(), 1);
        assert!(snap.outputs[0].available_modes.is_empty());
    }
}
