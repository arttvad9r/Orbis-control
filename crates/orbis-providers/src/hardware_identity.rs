//! Privacy-safe read-only DMI identity source for production diagnostics.

use std::fs;

use orbis_core::diagnostics::HardwareDiagnostics;
use orbis_core::identity::DeviceIdentity;

const SYS_VENDOR_PATH: &str = "/sys/class/dmi/id/sys_vendor";
const PRODUCT_NAME_PATH: &str = "/sys/class/dmi/id/product_name";
const BOARD_NAME_PATH: &str = "/sys/class/dmi/id/board_name";
const BIOS_VERSION_PATH: &str = "/sys/class/dmi/id/bios_version";
const BIOS_DATE_PATH: &str = "/sys/class/dmi/id/bios_date";

/// Read-only provider for privacy-safe hardware identity diagnostics.
///
/// Only vendor, product, board, BIOS version, and BIOS date are read. Serial,
/// UUID, asset-tag, and other device-unique identifiers are deliberately not
/// part of this provider's allowlist.
#[derive(Debug, Clone, Copy, Default)]
pub struct HardwareIdentityProvider;

impl HardwareIdentityProvider {
    /// Create a hardware identity provider.
    pub const fn new() -> Self {
        Self
    }

    /// Collect privacy-safe DMI identity when every required field is readable.
    ///
    /// `DeviceIdentity` has non-optional fields, so a partial DMI read is not
    /// padded with synthetic empty/default values. Any missing, unreadable, or
    /// blank required field leaves `identity` absent.
    pub fn snapshot(&self) -> HardwareDiagnostics {
        HardwareDiagnostics {
            identity: identity_from_lookup(read_trimmed_file),
        }
    }
}

fn identity_from_lookup(mut lookup: impl FnMut(&str) -> Option<String>) -> Option<DeviceIdentity> {
    Some(DeviceIdentity {
        vendor: normalized_value(lookup(SYS_VENDOR_PATH))?,
        product: normalized_value(lookup(PRODUCT_NAME_PATH))?,
        board: normalized_value(lookup(BOARD_NAME_PATH))?,
        bios_version: normalized_value(lookup(BIOS_VERSION_PATH))?,
        bios_date: normalized_value(lookup(BIOS_DATE_PATH))?,
    })
}

fn read_trimmed_file(path: &str) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .and_then(|value| normalized_value(Some(value)))
}

fn normalized_value(value: Option<String>) -> Option<String> {
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
    use std::{cell::RefCell, collections::HashMap};

    use super::*;

    fn complete_values() -> HashMap<&'static str, String> {
        HashMap::from([
            (SYS_VENDOR_PATH, " ASUSTeK COMPUTER INC.\n".into()),
            (PRODUCT_NAME_PATH, " ROG Zephyrus G14 ".into()),
            (BOARD_NAME_PATH, " GA403UI\n".into()),
            (BIOS_VERSION_PATH, " GA403UI.310\n".into()),
            (BIOS_DATE_PATH, " 07/18/2026\n".into()),
        ])
    }

    #[test]
    fn complete_identity_is_preserved_after_trimming() {
        let values = complete_values();
        let identity = identity_from_lookup(|path| values.get(path).cloned()).unwrap();

        assert_eq!(identity.vendor, "ASUSTeK COMPUTER INC.");
        assert_eq!(identity.product, "ROG Zephyrus G14");
        assert_eq!(identity.board, "GA403UI");
        assert_eq!(identity.bios_version, "GA403UI.310");
        assert_eq!(identity.bios_date, "07/18/2026");
    }

    #[test]
    fn missing_required_field_keeps_identity_absent() {
        let mut values = complete_values();
        values.remove(BOARD_NAME_PATH);

        assert!(identity_from_lookup(|path| values.get(path).cloned()).is_none());
    }

    #[test]
    fn blank_required_field_keeps_identity_absent() {
        let mut values = complete_values();
        values.insert(BIOS_VERSION_PATH, "   \n".into());

        assert!(identity_from_lookup(|path| values.get(path).cloned()).is_none());
    }

    #[test]
    fn lookup_is_strictly_limited_to_privacy_safe_paths() {
        let queried = RefCell::new(Vec::new());
        let values = complete_values();

        let identity = identity_from_lookup(|path| {
            queried.borrow_mut().push(path.to_owned());
            values.get(path).cloned()
        });

        assert!(identity.is_some());
        assert_eq!(
            queried.into_inner(),
            vec![
                SYS_VENDOR_PATH,
                PRODUCT_NAME_PATH,
                BOARD_NAME_PATH,
                BIOS_VERSION_PATH,
                BIOS_DATE_PATH,
            ]
        );
    }

    #[test]
    fn serial_number_is_never_requested() {
        let queried = RefCell::new(Vec::new());
        let values = complete_values();

        let identity = identity_from_lookup(|path| {
            queried.borrow_mut().push(path.to_owned());
            values.get(path).cloned()
        });

        assert!(identity.is_some());
        assert!(
            queried
                .into_inner()
                .iter()
                .all(|path| !path.to_ascii_lowercase().contains("serial"))
        );
    }

    #[test]
    fn snapshot_shape_keeps_identity_optional() {
        let values = complete_values();
        let diagnostics = HardwareDiagnostics {
            identity: identity_from_lookup(|path| values.get(path).cloned()),
        };

        assert!(diagnostics.identity.is_some());
    }
}
