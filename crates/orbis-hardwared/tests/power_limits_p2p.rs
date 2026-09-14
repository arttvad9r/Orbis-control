use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

use async_trait::async_trait;
use orbis_core::{action::ApplyResult, limits::PowerLimitField};
use orbis_hardwared::{
    AuthorizeError, Authorizer, handle_set_power_limit,
    power_limits::{PowerLimitMutationBackend, PowerLimitMutationReadback, wire},
};
use orbis_providers::error::ProviderError;

#[derive(Clone, Copy)]
enum Auth {
    Allow,
    Deny,
}

struct FakeAuth {
    outcome: Auth,
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl Authorizer for FakeAuth {
    async fn authorize(&self, _sender: &str) -> Result<(), AuthorizeError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        match self.outcome {
            Auth::Allow => Ok(()),
            Auth::Deny => Err(AuthorizeError::Denied("denied".into())),
        }
    }
}

#[derive(Clone)]
struct FakeBackend {
    result: Arc<std::sync::Mutex<Result<PowerLimitMutationReadback, String>>>,
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl PowerLimitMutationBackend for FakeBackend {
    async fn set_power_limit(
        &self,
        _field: PowerLimitField,
        _value: i32,
    ) -> Result<PowerLimitMutationReadback, ProviderError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let result = self.result.lock().unwrap().clone();
        match result {
            Ok(readback) => Ok(readback),
            Err(message) if message == "timeout" => {
                tokio::time::sleep(Duration::from_secs(3)).await;
                unreachable!()
            }
            Err(message) if message == "range" => Err(ProviderError::InvalidRequest(message)),
            Err(message) => Err(ProviderError::Conflict(message)),
        }
    }
}

fn applied(field: PowerLimitField, value: i32, observed: i32) -> PowerLimitMutationReadback {
    PowerLimitMutationReadback {
        field,
        requested: value,
        observed,
        result: ApplyResult::Applied,
    }
}

#[tokio::test]
async fn private_hardware1_success_and_readback_mismatch() {
    let auth_calls = Arc::new(AtomicUsize::new(0));
    let backend_calls = Arc::new(AtomicUsize::new(0));
    let backend = FakeBackend {
        result: Arc::new(std::sync::Mutex::new(Ok(applied(
            PowerLimitField::Spl,
            50,
            50,
        )))),
        calls: backend_calls.clone(),
    };
    let result = handle_set_power_limit(
        &FakeAuth {
            outcome: Auth::Allow,
            calls: auth_calls.clone(),
        },
        &backend,
        wire::SPL,
        50,
        ":1.1",
    )
    .await
    .unwrap();
    assert_eq!(result, 50);
    assert_eq!(auth_calls.load(Ordering::SeqCst), 1);
    assert_eq!(backend_calls.load(Ordering::SeqCst), 1);

    let mismatch_backend = FakeBackend {
        result: Arc::new(std::sync::Mutex::new(Ok(applied(
            PowerLimitField::Spl,
            50,
            45,
        )))),
        calls: Arc::new(AtomicUsize::new(0)),
    };
    let error = handle_set_power_limit(
        &FakeAuth {
            outcome: Auth::Allow,
            calls: Arc::new(AtomicUsize::new(0)),
        },
        &mismatch_backend,
        wire::SPL,
        50,
        ":1.1",
    )
    .await
    .unwrap_err();
    assert!(matches!(error, zbus::fdo::Error::Failed(_)));
}

#[tokio::test]
async fn invalid_range_is_rejected_before_authorization_or_backend() {
    let auth_calls = Arc::new(AtomicUsize::new(0));
    let backend_calls = Arc::new(AtomicUsize::new(0));
    let backend = FakeBackend {
        result: Arc::new(std::sync::Mutex::new(Err("range".into()))),
        calls: backend_calls.clone(),
    };
    let error = handle_set_power_limit(
        &FakeAuth {
            outcome: Auth::Allow,
            calls: auth_calls.clone(),
        },
        &backend,
        wire::SPL,
        90,
        ":1.1",
    )
    .await
    .unwrap_err();
    assert!(matches!(error, zbus::fdo::Error::InvalidArgs(_)));
}

#[tokio::test]
async fn authorization_failure_timeout_and_unsupported_are_honest() {
    let calls = Arc::new(AtomicUsize::new(0));
    let backend = FakeBackend {
        result: Arc::new(std::sync::Mutex::new(Ok(applied(
            PowerLimitField::Spl,
            50,
            50,
        )))),
        calls: calls.clone(),
    };
    let denied = handle_set_power_limit(
        &FakeAuth {
            outcome: Auth::Deny,
            calls: Arc::new(AtomicUsize::new(0)),
        },
        &backend,
        wire::SPL,
        50,
        ":1.1",
    )
    .await
    .unwrap_err();
    assert!(matches!(denied, zbus::fdo::Error::AccessDenied(_)));
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    let timeout_backend = FakeBackend {
        result: Arc::new(std::sync::Mutex::new(Err("timeout".into()))),
        calls: Arc::new(AtomicUsize::new(0)),
    };
    let timeout = handle_set_power_limit(
        &FakeAuth {
            outcome: Auth::Allow,
            calls: Arc::new(AtomicUsize::new(0)),
        },
        &timeout_backend,
        wire::SPL,
        50,
        ":1.1",
    )
    .await
    .unwrap_err();
    assert!(matches!(timeout, zbus::fdo::Error::Failed(message) if message.contains("timeout")));

    let unsupported = handle_set_power_limit(
        &FakeAuth {
            outcome: Auth::Allow,
            calls: Arc::new(AtomicUsize::new(0)),
        },
        &backend,
        9,
        50,
        ":1.1",
    )
    .await
    .unwrap_err();
    assert!(matches!(unsupported, zbus::fdo::Error::InvalidArgs(_)));
}
