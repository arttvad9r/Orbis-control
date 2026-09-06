use std::path::{Path, PathBuf};

use orbis_core::action::ApplyResult;
use orbis_providers::error::ProviderError;

pub const ASPM_POLICY_PATH: &str = "/sys/module/pcie_aspm/parameters/policy";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AspmMutationStatus {
    Supported,
    Unsupported,
    TemporarilyUnavailable,
    PermissionDenied,
    Unknown,
}

pub mod mutation_wire {
    use super::AspmMutationStatus;

    pub const SUPPORTED: u8 = 0;
    pub const UNSUPPORTED: u8 = 1;
    pub const TEMPORARILY_UNAVAILABLE: u8 = 2;
    pub const PERMISSION_DENIED: u8 = 3;
    pub const UNKNOWN: u8 = 4;

    pub fn to_wire(status: AspmMutationStatus) -> u8 {
        match status {
            AspmMutationStatus::Supported => SUPPORTED,
            AspmMutationStatus::Unsupported => UNSUPPORTED,
            AspmMutationStatus::TemporarilyUnavailable => TEMPORARILY_UNAVAILABLE,
            AspmMutationStatus::PermissionDenied => PERMISSION_DENIED,
            AspmMutationStatus::Unknown => UNKNOWN,
        }
    }
}

pub trait AspmIo: Send + Sync {
    fn read_to_string(&self, path: &Path) -> Result<String, ProviderError>;
    fn write(&self, path: &Path, content: &str) -> Result<(), ProviderError>;
}

pub struct StdAspmIo;

impl AspmIo for StdAspmIo {
    fn read_to_string(&self, path: &Path) -> Result<String, ProviderError> {
        std::fs::read_to_string(path).map_err(ProviderError::Io)
    }

    fn write(&self, path: &Path, content: &str) -> Result<(), ProviderError> {
        std::fs::write(path, content).map_err(ProviderError::Io)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AspmPolicy {
    Default,
    Performance,
    Powersave,
    Powersupersave,
}

impl AspmPolicy {
    fn parse(raw: &str) -> Result<Self, ProviderError> {
        let active = raw
            .split_whitespace()
            .find_map(|token| token.strip_prefix('[').and_then(|v| v.strip_suffix(']')))
            .unwrap_or(raw.trim());
        match active {
            "default" => Ok(Self::Default),
            "performance" => Ok(Self::Performance),
            "powersave" => Ok(Self::Powersave),
            "powersupersave" => Ok(Self::Powersupersave),
            other => Err(ProviderError::Internal(format!(
                "hardwared: unknown ASPM policy '{other}'"
            ))),
        }
    }

    fn symbol(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Performance => "performance",
            Self::Powersave => "powersave",
            Self::Powersupersave => "powersupersave",
        }
    }
}

pub struct AspmWriter<S: AspmIo> {
    pub(crate) io: S,
    policy_path: PathBuf,
}

impl Default for AspmWriter<StdAspmIo> {
    fn default() -> Self {
        Self::with_io(StdAspmIo, PathBuf::from(ASPM_POLICY_PATH))
    }
}

impl<S: AspmIo> AspmWriter<S> {
    pub(crate) fn with_io(io: S, policy_path: PathBuf) -> Self {
        Self { io, policy_path }
    }

    pub fn current_policy(&self) -> Result<AspmPolicy, ProviderError> {
        AspmPolicy::parse(&self.io.read_to_string(&self.policy_path)?)
    }

    pub fn mutation_status(&self) -> AspmMutationStatus {
        match self.current_policy() {
            Ok(_) => AspmMutationStatus::Supported,
            Err(ProviderError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                AspmMutationStatus::Unsupported
            }
            Err(ProviderError::Io(error))
                if error.kind() == std::io::ErrorKind::PermissionDenied =>
            {
                AspmMutationStatus::PermissionDenied
            }
            Err(ProviderError::Io(_)) => AspmMutationStatus::TemporarilyUnavailable,
            Err(_) => AspmMutationStatus::Unknown,
        }
    }

    pub fn set_disabled(&self, disabled: bool) -> Result<ApplyResult, ProviderError> {
        let requested = if disabled {
            AspmPolicy::Performance
        } else {
            AspmPolicy::Default
        };
        self.io
            .write(&self.policy_path, &format!("{}\n", requested.symbol()))?;
        if self.current_policy()? != requested {
            return Err(ProviderError::Conflict(format!(
                "hardwared: ASPM read-back did not confirm '{}'",
                requested.symbol()
            )));
        }
        Ok(ApplyResult::Applied)
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    use orbis_core::action::ApplyResult;
    use orbis_providers::error::ProviderError;

    use super::{AspmIo, AspmPolicy, AspmWriter};

    struct FakeIo {
        state: Mutex<(String, Vec<String>)>,
    }

    impl AspmIo for FakeIo {
        fn read_to_string(&self, _path: &Path) -> Result<String, ProviderError> {
            Ok(self.state.lock().unwrap().0.clone())
        }

        fn write(&self, _path: &Path, content: &str) -> Result<(), ProviderError> {
            let mut state = self.state.lock().unwrap();
            state.1.push(content.to_string());
            state.0 = format!("[{}] default powersave", content.trim());
            Ok(())
        }
    }

    #[test]
    fn disabling_aspm_writes_performance_and_confirms_readback() {
        let io = FakeIo {
            state: Mutex::new(("[default] performance powersave".into(), Vec::new())),
        };
        let writer = AspmWriter::with_io(io, PathBuf::from("policy"));

        assert_eq!(writer.set_disabled(true).unwrap(), ApplyResult::Applied);
        assert_eq!(writer.io.state.lock().unwrap().1, vec!["performance\n"]);
        assert_eq!(writer.current_policy().unwrap(), AspmPolicy::Performance);
    }
}
