//! AMD Ryzen Curve Optimizer through the fixed `ryzenadj` invocation.
//!
//! The executable path, the SMU sysfs directory and every argument shape are
//! fixed here; callers only choose one bounded integer. `ryzenadj` exits with
//! success only after the SMU acknowledged the request, but the tool has no
//! read-back for the Curve Optimizer, so a success means "acknowledged by the
//! SMU", not "observed".

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::error::ProviderError;

/// Fixed executable.
pub const RYZENADJ_EXECUTABLE: &str = "/usr/bin/ryzenadj";
/// Directory the `ryzen_smu` kernel module exposes.
pub const SMU_SYSFS_ROOT: &str = "/sys/kernel/ryzen_smu_drv";
/// Deepest all-core undervolt offered.
pub const CURVE_OPTIMIZER_MIN: i32 = -30;
/// Highest offset offered; positive (overvolt) offsets are not.
pub const CURVE_OPTIMIZER_MAX: i32 = 0;

const RUN_TIMEOUT: Duration = Duration::from_secs(10);

/// Result of one `ryzenadj` run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RyzenAdjOutput {
    /// True when the process exited with status 0.
    pub success: bool,
    /// Captured standard output and error, for diagnostics.
    pub text: String,
}

/// Runs `ryzenadj` with already-validated arguments.
pub trait RyzenAdjRunner: Send + Sync {
    /// Run the fixed executable with these arguments.
    fn run(&self, args: &[String]) -> Result<RyzenAdjOutput, ProviderError>;
}

/// Production runner: the fixed executable, no shell, bounded time and output.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemRyzenAdj;

impl RyzenAdjRunner for SystemRyzenAdj {
    fn run(&self, args: &[String]) -> Result<RyzenAdjOutput, ProviderError> {
        let mut child = Command::new(RYZENADJ_EXECUTABLE)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| match error.kind() {
                std::io::ErrorKind::NotFound => {
                    ProviderError::Unsupported("ryzenadj не установлен".into())
                }
                _ => ProviderError::Io(error),
            })?;
        let started = Instant::now();
        let status = loop {
            match child.try_wait().map_err(ProviderError::Io)? {
                Some(status) => break status,
                None if started.elapsed() >= RUN_TIMEOUT => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(ProviderError::Timeout(
                        "ryzenadj не ответил; исход записи неизвестен".into(),
                    ));
                }
                None => std::thread::sleep(Duration::from_millis(20)),
            }
        };
        let mut text = String::new();
        for stream in [
            child.stdout.take().map(|s| Box::new(s) as Box<dyn Read>),
            child.stderr.take().map(|s| Box::new(s) as Box<dyn Read>),
        ]
        .into_iter()
        .flatten()
        {
            let _ = stream.take(16 * 1024).read_to_string(&mut text);
        }
        Ok(RyzenAdjOutput {
            success: status.success(),
            text,
        })
    }
}

/// Cheap, unprivileged evidence that the tool and the SMU driver are installed.
pub fn is_installed(executable: &Path, smu_root: &Path) -> bool {
    executable.is_file() && smu_root.join("version").is_file()
}

/// `--set-coall` takes an unsigned 32-bit value; a negative offset is encoded
/// as `0x100000 + offset`.
pub fn curve_optimizer_argument(offset: i32) -> Result<String, ProviderError> {
    if !(CURVE_OPTIMIZER_MIN..=CURVE_OPTIMIZER_MAX).contains(&offset) {
        return Err(ProviderError::InvalidRequest(format!(
            "Curve Optimizer {offset} вне допустимого диапазона {CURVE_OPTIMIZER_MIN}..{CURVE_OPTIMIZER_MAX}"
        )));
    }
    let raw = if offset < 0 {
        0x10_0000 + offset
    } else {
        offset
    };
    Ok(format!("--set-coall={raw}"))
}

/// Read-only probe: the SMU answers `--info` for this CPU.
pub fn probe(runner: &dyn RyzenAdjRunner) -> bool {
    runner
        .run(&["--info".to_string()])
        .is_ok_and(|output| output.success && output.text.contains("PM Table Version"))
}

/// Set the all-core Curve Optimizer offset. Returns the offset the SMU
/// acknowledged.
pub fn apply_curve_optimizer(
    runner: &dyn RyzenAdjRunner,
    offset: i32,
) -> Result<i32, ProviderError> {
    let argument = curve_optimizer_argument(offset)?;
    let output = runner.run(&[argument])?;
    if output.text.contains("rejected by SMU") {
        return Err(ProviderError::Unsupported(format!(
            "прошивка не поддерживает Curve Optimizer: {}",
            output.text.trim()
        )));
    }
    if !output.success {
        let detail: String = output.text.trim().chars().take(200).collect();
        return Err(ProviderError::Internal(format!(
            "ryzenadj отклонил Curve Optimizer {offset}: {detail}"
        )));
    }
    Ok(offset)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct Recorder(Mutex<Vec<Vec<String>>>, bool);

    impl RyzenAdjRunner for Recorder {
        fn run(&self, args: &[String]) -> Result<RyzenAdjOutput, ProviderError> {
            self.0.lock().unwrap().push(args.to_vec());
            Ok(RyzenAdjOutput {
                success: self.1,
                text: "PM Table Version: 450005".into(),
            })
        }
    }

    #[test]
    fn negative_offsets_are_encoded_as_twos_complement_below_0x100000() {
        assert_eq!(curve_optimizer_argument(0).unwrap(), "--set-coall=0");
        assert_eq!(curve_optimizer_argument(-1).unwrap(), "--set-coall=1048575");
        assert_eq!(
            curve_optimizer_argument(-30).unwrap(),
            "--set-coall=1048546"
        );
    }

    #[test]
    fn out_of_range_offsets_never_reach_the_tool() {
        let runner = Recorder(Mutex::new(Vec::new()), true);
        for offset in [-31, 1, i32::MIN, i32::MAX] {
            assert!(matches!(
                apply_curve_optimizer(&runner, offset),
                Err(ProviderError::InvalidRequest(_))
            ));
        }
        assert!(runner.0.lock().unwrap().is_empty());
    }

    #[test]
    fn an_smu_rejection_means_the_firmware_has_no_curve_optimizer() {
        struct Rejecting;
        impl RyzenAdjRunner for Rejecting {
            fn run(&self, _: &[String]) -> Result<RyzenAdjOutput, ProviderError> {
                Ok(RyzenAdjOutput {
                    success: false,
                    text: "set_coall is rejected by SMU".into(),
                })
            }
        }
        assert!(matches!(
            apply_curve_optimizer(&Rejecting, -5),
            Err(ProviderError::Unsupported(_))
        ));
    }

    #[test]
    fn a_rejected_write_is_an_error_and_an_accepted_one_uses_fixed_arguments() {
        let rejecting = Recorder(Mutex::new(Vec::new()), false);
        assert!(apply_curve_optimizer(&rejecting, -10).is_err());
        let accepting = Recorder(Mutex::new(Vec::new()), true);
        assert_eq!(apply_curve_optimizer(&accepting, -10).unwrap(), -10);
        assert_eq!(
            accepting.0.lock().unwrap().as_slice(),
            [vec!["--set-coall=1048566".to_string()]]
        );
        assert!(probe(&accepting));
        assert!(!probe(&rejecting));
    }
}
