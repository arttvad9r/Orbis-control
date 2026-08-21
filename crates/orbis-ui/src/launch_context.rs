//! Testable launch policy for the GUI entrypoint.

use std::fmt;

/// The kind of process being launched by the UI binary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LaunchMode {
    Interactive,
    Screenshot,
    #[cfg(test)]
    OffscreenTest,
}

/// Process context used before any preferences, runtime, or bus setup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LaunchContext {
    pub(crate) mode: LaunchMode,
    pub(crate) effective_uid: u32,
}

impl LaunchContext {
    pub(crate) const fn new(mode: LaunchMode, effective_uid: u32) -> Self {
        Self {
            mode,
            effective_uid,
        }
    }

    pub(crate) const fn allows_root(self) -> bool {
        !matches!(self.mode, LaunchMode::Interactive)
    }

    pub(crate) fn validate(self) -> Result<(), RootLaunchError> {
        if self.effective_uid == 0 && !self.allows_root() {
            return Err(RootLaunchError);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RootLaunchError;

impl fmt::Display for RootLaunchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(
            "refusing interactive GUI launch with effective UID 0; start Orbis Control as a normal user",
        )
    }
}

impl std::error::Error for RootLaunchError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interactive_root_launch_is_rejected() {
        let context = LaunchContext::new(LaunchMode::Interactive, 0);

        assert_eq!(context.validate(), Err(RootLaunchError));
    }

    #[test]
    fn normal_user_interactive_launch_is_allowed() {
        let context = LaunchContext::new(LaunchMode::Interactive, 1000);

        assert_eq!(context.validate(), Ok(()));
    }

    #[test]
    fn root_screenshot_and_offscreen_launches_are_allowed() {
        assert_eq!(
            LaunchContext::new(LaunchMode::Screenshot, 0).validate(),
            Ok(())
        );
        assert_eq!(
            LaunchContext::new(LaunchMode::OffscreenTest, 0).validate(),
            Ok(())
        );
    }
}
