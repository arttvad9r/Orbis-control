//! Compile-time application metadata source for production diagnostics.

use orbis_core::diagnostics::ApplicationDiagnostics;

/// Compile-time environment variable used for an explicitly embedded build revision.
pub const BUILD_REVISION_ENV: &str = "ORBIS_BUILD_REVISION";

/// Compile-time environment variable used for an explicitly embedded build channel.
pub const BUILD_CHANNEL_ENV: &str = "ORBIS_BUILD_CHANNEL";

/// Build immutable application metadata for diagnostics.
///
/// The package version comes from Cargo metadata for the `orbis-ui` package,
/// which also owns the `orbis-control` binary target. Optional revision/channel
/// values are accepted only when deliberately embedded at compile time. This
/// function never executes `git` or any other runtime command.
pub fn application_diagnostics() -> ApplicationDiagnostics {
    application_diagnostics_from(
        env!("CARGO_PKG_VERSION"),
        option_env!("ORBIS_BUILD_REVISION"),
        option_env!("ORBIS_BUILD_CHANNEL"),
    )
}

fn application_diagnostics_from(
    package_version: &str,
    build_revision: Option<&str>,
    build_channel: Option<&str>,
) -> ApplicationDiagnostics {
    ApplicationDiagnostics {
        package_version: package_version.to_owned(),
        build_revision: optional_build_value(build_revision),
        build_channel: optional_build_value(build_channel),
    }
}

fn optional_build_value(value: Option<&str>) -> Option<String> {
    value.and_then(|value| {
        let value = value.trim();
        if value.is_empty() {
            None
        } else {
            Some(value.to_owned())
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_version_is_exact_cargo_package_version() {
        let metadata = application_diagnostics();
        assert_eq!(metadata.package_version, env!("CARGO_PKG_VERSION"));
        assert!(!metadata.package_version.is_empty());
    }

    #[test]
    fn absent_optional_build_metadata_remains_absent() {
        let metadata = application_diagnostics_from("1.2.3", None, None);
        assert_eq!(metadata.package_version, "1.2.3");
        assert!(metadata.build_revision.is_none());
        assert!(metadata.build_channel.is_none());

        let blank = application_diagnostics_from("1.2.3", Some("  "), Some("\t"));
        assert!(blank.build_revision.is_none());
        assert!(blank.build_channel.is_none());
    }

    #[test]
    fn explicitly_embedded_optional_build_metadata_is_preserved() {
        let metadata = application_diagnostics_from("1.2.3", Some(" abcdef123 "), Some(" beta "));
        assert_eq!(metadata.build_revision.as_deref(), Some("abcdef123"));
        assert_eq!(metadata.build_channel.as_deref(), Some("beta"));
    }

    #[test]
    fn production_source_matches_optional_compile_time_metadata() {
        let metadata = application_diagnostics();
        assert_eq!(
            metadata.build_revision,
            optional_build_value(option_env!("ORBIS_BUILD_REVISION"))
        );
        assert_eq!(
            metadata.build_channel,
            optional_build_value(option_env!("ORBIS_BUILD_CHANNEL"))
        );
    }
}
