//! Keyboard backlight auto-off: after the configured input idleness for the
//! current power source the backlight is switched off, and the previous level is
//! restored on the next input. Nothing here writes at load time; only observed
//! idleness dims, and a level changed by the user meanwhile (Fn keys) wins.

use std::sync::Arc;
use std::time::Duration;

use orbis_config::KeyboardTimeout;
use orbis_providers::{IdleEvent, IdleSource, IdleWatch, KeyboardLight, ProviderError};
use tokio::sync::watch;

/// What the auto-off is doing, for the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyboardTimeoutStatus {
    /// No timeout for the current power source.
    Off,
    /// Waiting for the desktop to report idleness.
    Waiting,
    /// The backlight is off because of idleness.
    Dimmed,
    /// The feature cannot work right now.
    Unavailable(String),
}

fn wanted(
    settings: &watch::Receiver<KeyboardTimeout>,
    ac: &watch::Receiver<Option<bool>>,
) -> Option<u32> {
    let ac_online = (*ac.borrow())?;
    settings
        .borrow()
        .for_source(ac_online)
        .filter(|secs| *secs > 0)
}

struct Runner<F> {
    idle: Arc<dyn IdleSource>,
    light: Arc<dyn KeyboardLight>,
    report: F,
    watch: Option<IdleWatch>,
    dimmed: Option<u8>,
    timeout: Option<u32>,
}

impl<F: FnMut(KeyboardTimeoutStatus)> Runner<F> {
    fn idle_status(&self) -> KeyboardTimeoutStatus {
        if self.watch.is_some() {
            KeyboardTimeoutStatus::Waiting
        } else {
            KeyboardTimeoutStatus::Off
        }
    }

    fn fail(&mut self, what: &str, error: &ProviderError) {
        tracing::warn!(?error, "keyboard timeout: {what}");
        (self.report)(KeyboardTimeoutStatus::Unavailable(format!(
            "{what}: {error}"
        )));
    }

    async fn retarget(&mut self, timeout: Option<u32>) {
        self.watch = None;
        self.restore().await;
        self.timeout = timeout;
        match timeout {
            None => (self.report)(KeyboardTimeoutStatus::Off),
            Some(secs) => match self.idle.watch(Duration::from_secs(u64::from(secs))).await {
                Ok(watch) => {
                    self.watch = Some(watch);
                    (self.report)(KeyboardTimeoutStatus::Waiting);
                }
                Err(error) => self.fail("отслеживание бездействия недоступно", &error),
            },
        }
    }

    async fn dim(&mut self) {
        if self.dimmed.is_some() {
            return;
        }
        let level = match self.light.level().await {
            Ok(level) => level,
            Err(error) => return self.fail("не удалось прочитать яркость", &error),
        };
        if level == 0 {
            return;
        }
        if let Err(error) = self.light.set_level(0).await {
            return self.fail("не удалось погасить подсветку", &error);
        }
        match self.light.level().await {
            Ok(0) => {
                self.dimmed = Some(level);
                (self.report)(KeyboardTimeoutStatus::Dimmed);
            }
            Ok(other) => self.fail(
                "подсветка не погасла",
                &ProviderError::Conflict(format!("уровень {other} после записи 0")),
            ),
            Err(error) => self.fail("не удалось подтвердить погашение", &error),
        }
    }

    async fn restore(&mut self) {
        let Some(saved) = self.dimmed.take() else {
            return;
        };
        match self.light.level().await {
            Ok(0) => {
                if let Err(error) = self.light.set_level(saved).await {
                    return self.fail("не удалось вернуть подсветку", &error);
                }
                match self.light.level().await {
                    Ok(level) if level == saved => {}
                    Ok(level) => {
                        return self.fail(
                            "подсветка не вернулась",
                            &ProviderError::Conflict(format!("уровень {level}, ожидался {saved}")),
                        );
                    }
                    Err(error) => return self.fail("не удалось подтвердить возврат", &error),
                }
            }
            Ok(_) => {}
            Err(error) => return self.fail("не удалось прочитать яркость", &error),
        }
        let status = self.idle_status();
        (self.report)(status);
    }

    async fn next_event(&mut self) -> Option<IdleEvent> {
        match &mut self.watch {
            Some(watch) => watch.next().await,
            None => std::future::pending().await,
        }
    }
}

/// Status line for the UI and whether it describes a problem.
pub fn status_text(status: &KeyboardTimeoutStatus) -> (String, bool) {
    match status {
        KeyboardTimeoutStatus::Off => ("Для текущего источника питания не задано".into(), false),
        KeyboardTimeoutStatus::Waiting => ("Ожидание бездействия".into(), false),
        KeyboardTimeoutStatus::Dimmed => ("Подсветка погашена из-за бездействия".into(), false),
        KeyboardTimeoutStatus::Unavailable(reason) => (reason.clone(), true),
    }
}

/// Run until `shutdown` fires (or is dropped) or both inputs close; restores a
/// dimmed backlight before returning.
pub async fn run_keyboard_timeout<F>(
    mut settings: watch::Receiver<KeyboardTimeout>,
    mut ac_online: watch::Receiver<Option<bool>>,
    mut shutdown: tokio::sync::oneshot::Receiver<()>,
    idle: Arc<dyn IdleSource>,
    light: Arc<dyn KeyboardLight>,
    report: F,
) where
    F: FnMut(KeyboardTimeoutStatus),
{
    let mut runner = Runner {
        idle,
        light,
        report,
        watch: None,
        dimmed: None,
        timeout: None,
    };
    (runner.report)(KeyboardTimeoutStatus::Off);
    loop {
        let target = wanted(&settings, &ac_online);
        if target != runner.timeout {
            runner.retarget(target).await;
        }
        tokio::select! {
            _ = &mut shutdown => break,
            changed = settings.changed() => if changed.is_err() { break },
            changed = ac_online.changed() => if changed.is_err() { break },
            event = runner.next_event() => match event {
                Some(IdleEvent::Idle) => runner.dim().await,
                Some(IdleEvent::Resumed) => runner.restore().await,
                None => {
                    runner.watch = None;
                    (runner.report)(KeyboardTimeoutStatus::Unavailable(
                        "соединение с рабочим столом потеряно".into(),
                    ));
                }
            },
        }
    }
    runner.restore().await;
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use async_trait::async_trait;
    use orbis_providers::IdleEvent;
    use tokio::sync::mpsc;

    use super::*;

    struct FakeIdle {
        opened: mpsc::UnboundedSender<(Duration, mpsc::UnboundedSender<IdleEvent>)>,
        fail: bool,
    }

    #[async_trait]
    impl IdleSource for FakeIdle {
        async fn watch(&self, timeout: Duration) -> Result<IdleWatch, ProviderError> {
            if self.fail {
                return Err(ProviderError::Unsupported("no idle protocol".into()));
            }
            let (watch, sender) = IdleWatch::channel();
            self.opened.send((timeout, sender)).unwrap();
            Ok(watch)
        }
    }

    struct FakeLight {
        level: Mutex<u8>,
        writes: Mutex<Vec<u8>>,
        ignore_writes: bool,
    }

    impl FakeLight {
        fn new(level: u8) -> Arc<Self> {
            Arc::new(Self {
                level: Mutex::new(level),
                writes: Mutex::new(Vec::new()),
                ignore_writes: false,
            })
        }
    }

    #[async_trait]
    impl KeyboardLight for FakeLight {
        async fn level(&self) -> Result<u8, ProviderError> {
            Ok(*self.level.lock().unwrap())
        }
        async fn set_level(&self, level: u8) -> Result<(), ProviderError> {
            self.writes.lock().unwrap().push(level);
            if !self.ignore_writes {
                *self.level.lock().unwrap() = level;
            }
            Ok(())
        }
    }

    struct Harness {
        settings: watch::Sender<KeyboardTimeout>,
        ac: watch::Sender<Option<bool>>,
        statuses: mpsc::UnboundedReceiver<KeyboardTimeoutStatus>,
        opened: mpsc::UnboundedReceiver<(Duration, mpsc::UnboundedSender<IdleEvent>)>,
        light: Arc<FakeLight>,
        stop: tokio::sync::oneshot::Sender<()>,
        task: tokio::task::JoinHandle<()>,
    }

    fn start(
        light: Arc<FakeLight>,
        settings: KeyboardTimeout,
        ac: Option<bool>,
        fail: bool,
    ) -> Harness {
        let (settings_tx, settings_rx) = watch::channel(settings);
        let (ac_tx, ac_rx) = watch::channel(ac);
        let (opened_tx, opened) = mpsc::unbounded_channel();
        let (status_tx, statuses) = mpsc::unbounded_channel();
        let (stop, stop_rx) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(run_keyboard_timeout(
            settings_rx,
            ac_rx,
            stop_rx,
            Arc::new(FakeIdle {
                opened: opened_tx,
                fail,
            }),
            light.clone(),
            move |status| {
                let _ = status_tx.send(status);
            },
        ));
        Harness {
            settings: settings_tx,
            ac: ac_tx,
            statuses,
            opened,
            light,
            stop,
            task,
        }
    }

    async fn status(harness: &mut Harness) -> KeyboardTimeoutStatus {
        tokio::time::timeout(Duration::from_secs(2), harness.statuses.recv())
            .await
            .expect("status in time")
            .expect("task alive")
    }

    async fn watch_opened(harness: &mut Harness) -> (Duration, mpsc::UnboundedSender<IdleEvent>) {
        tokio::time::timeout(Duration::from_secs(2), harness.opened.recv())
            .await
            .expect("watch opened in time")
            .expect("task alive")
    }

    const AC_30: KeyboardTimeout = KeyboardTimeout {
        ac_secs: Some(30),
        battery_secs: None,
    };

    #[tokio::test]
    async fn idle_dims_and_input_restores_the_previous_level() {
        let mut h = start(FakeLight::new(2), AC_30, Some(true), false);
        assert_eq!(status(&mut h).await, KeyboardTimeoutStatus::Off);
        assert_eq!(status(&mut h).await, KeyboardTimeoutStatus::Waiting);
        let (timeout, events) = watch_opened(&mut h).await;
        assert_eq!(timeout, Duration::from_secs(30));
        assert!(h.light.writes.lock().unwrap().is_empty());

        events.send(IdleEvent::Idle).unwrap();
        assert_eq!(status(&mut h).await, KeyboardTimeoutStatus::Dimmed);
        assert_eq!(*h.light.level.lock().unwrap(), 0);

        events.send(IdleEvent::Resumed).unwrap();
        assert_eq!(status(&mut h).await, KeyboardTimeoutStatus::Waiting);
        assert_eq!(*h.light.level.lock().unwrap(), 2);
        assert_eq!(*h.light.writes.lock().unwrap(), vec![0, 2]);
    }

    #[tokio::test]
    async fn a_level_chosen_while_dimmed_is_not_overwritten() {
        let mut h = start(FakeLight::new(3), AC_30, Some(true), false);
        status(&mut h).await;
        status(&mut h).await;
        let (_, events) = watch_opened(&mut h).await;
        events.send(IdleEvent::Idle).unwrap();
        assert_eq!(status(&mut h).await, KeyboardTimeoutStatus::Dimmed);
        *h.light.level.lock().unwrap() = 1;
        events.send(IdleEvent::Resumed).unwrap();
        assert_eq!(status(&mut h).await, KeyboardTimeoutStatus::Waiting);
        assert_eq!(*h.light.level.lock().unwrap(), 1);
        assert_eq!(*h.light.writes.lock().unwrap(), vec![0]);
    }

    #[tokio::test]
    async fn already_off_backlight_is_left_alone() {
        let mut h = start(FakeLight::new(0), AC_30, Some(true), false);
        status(&mut h).await;
        status(&mut h).await;
        let (_, events) = watch_opened(&mut h).await;
        events.send(IdleEvent::Idle).unwrap();
        events.send(IdleEvent::Resumed).unwrap();
        drop(events);
        h.settings.send_replace(KeyboardTimeout::default());
        assert_eq!(status(&mut h).await, KeyboardTimeoutStatus::Off);
        assert!(h.light.writes.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn switching_to_a_source_without_timeout_restores_the_backlight() {
        let mut h = start(FakeLight::new(2), AC_30, Some(true), false);
        status(&mut h).await;
        status(&mut h).await;
        let (_, events) = watch_opened(&mut h).await;
        events.send(IdleEvent::Idle).unwrap();
        assert_eq!(status(&mut h).await, KeyboardTimeoutStatus::Dimmed);
        h.ac.send_replace(Some(false));
        assert_eq!(status(&mut h).await, KeyboardTimeoutStatus::Off);
        assert_eq!(*h.light.level.lock().unwrap(), 2);
    }

    #[tokio::test]
    async fn unknown_power_source_never_dims() {
        let mut h = start(FakeLight::new(2), AC_30, None, false);
        assert_eq!(status(&mut h).await, KeyboardTimeoutStatus::Off);
        h.ac.send_replace(Some(true));
        assert_eq!(status(&mut h).await, KeyboardTimeoutStatus::Waiting);
    }

    #[tokio::test]
    async fn stopping_the_task_restores_a_dimmed_backlight() {
        let mut h = start(FakeLight::new(2), AC_30, Some(true), false);
        status(&mut h).await;
        status(&mut h).await;
        let (_, events) = watch_opened(&mut h).await;
        events.send(IdleEvent::Idle).unwrap();
        assert_eq!(status(&mut h).await, KeyboardTimeoutStatus::Dimmed);
        let Harness {
            stop, task, light, ..
        } = h;
        stop.send(()).unwrap();
        task.await.unwrap();
        assert_eq!(*light.level.lock().unwrap(), 2);
    }

    #[tokio::test]
    async fn missing_idle_support_and_ignored_writes_are_reported_honestly() {
        let mut h = start(FakeLight::new(2), AC_30, Some(true), true);
        status(&mut h).await;
        assert!(matches!(
            status(&mut h).await,
            KeyboardTimeoutStatus::Unavailable(_)
        ));

        let stuck = Arc::new(FakeLight {
            level: Mutex::new(2),
            writes: Mutex::new(Vec::new()),
            ignore_writes: true,
        });
        let mut h = start(stuck, AC_30, Some(true), false);
        status(&mut h).await;
        status(&mut h).await;
        let (_, events) = watch_opened(&mut h).await;
        events.send(IdleEvent::Idle).unwrap();
        assert!(matches!(
            status(&mut h).await,
            KeyboardTimeoutStatus::Unavailable(_)
        ));
        events.send(IdleEvent::Resumed).unwrap();
        assert_eq!(*h.light.writes.lock().unwrap(), vec![0]);
    }
}
