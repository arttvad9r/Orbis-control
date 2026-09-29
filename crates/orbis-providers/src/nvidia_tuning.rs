//! NVIDIA dGPU clock offsets and power limit through the NVML driver library.
//!
//! Reading needs no privileges; writing is only reachable from the privileged
//! `orbis-hardwared`. The driver library is loaded at run time, so a machine
//! without the proprietary driver simply reports the GPU as absent.

use std::fs;
use std::path::Path;

use nvml_wrapper::enum_wrappers::device::{Clock, PerformanceState};
use nvml_wrapper::error::NvmlError;
use nvml_wrapper::{Device, Nvml};
use orbis_core::nvidia_tuning::{
    NvidiaAvailability, NvidiaField, NvidiaSetting, NvidiaTuningState,
};

use crate::error::ProviderError;

/// Kernel sysfs root used by production.
pub const SYSFS_ROOT: &str = "/sys";

/// Driver access for one setting; the production implementation is [`Nvml`].
pub trait NvidiaTuningDriver: Send + Sync {
    /// Current value with the bounds the driver accepts.
    fn read(&self, field: NvidiaField) -> Result<NvidiaSetting, ProviderError>;
    /// All settings in [`NvidiaField::ALL`] order; a driver may share one session.
    fn read_all(&self) -> [Result<NvidiaSetting, ProviderError>; 3] {
        NvidiaField::ALL.map(|field| self.read(field))
    }
    /// Ask the driver to change one setting.
    fn write(&self, field: NvidiaField, value: i32) -> Result<(), ProviderError>;
}

/// Runtime state of the NVIDIA GPU as seen through sysfs, without waking it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuPresence {
    /// No PCI display device is bound to the proprietary `nvidia` driver.
    Absent,
    /// Bound and runtime-suspended.
    Asleep,
    /// Bound and awake.
    Awake,
}

/// Look for a display device bound to the `nvidia` driver.
pub fn gpu_presence(sysfs_root: &Path) -> GpuPresence {
    let Ok(devices) = fs::read_dir(sysfs_root.join("bus/pci/devices")) else {
        return GpuPresence::Absent;
    };
    let mut presence = GpuPresence::Absent;
    for device in devices.filter_map(Result::ok) {
        let path = device.path();
        let read = |name: &str| fs::read_to_string(path.join(name)).unwrap_or_default();
        let bound = fs::read_link(path.join("driver"))
            .ok()
            .and_then(|target| target.file_name().map(|name| name.to_os_string()))
            .is_some_and(|name| name == "nvidia");
        if !bound || read("vendor").trim() != "0x10de" || !read("class").trim().starts_with("0x03")
        {
            continue;
        }
        if read("power/runtime_status").trim() == "suspended" {
            presence = GpuPresence::Asleep;
        } else {
            return GpuPresence::Awake;
        }
    }
    presence
}

/// Cheap availability check that never wakes the GPU.
pub fn availability(sysfs_root: &Path) -> NvidiaAvailability {
    match gpu_presence(sysfs_root) {
        GpuPresence::Absent => NvidiaAvailability::Absent,
        GpuPresence::Asleep => NvidiaAvailability::Asleep,
        GpuPresence::Awake => NvidiaAvailability::Ready,
    }
}

/// Read every setting the driver exposes. An asleep GPU is reported as such and
/// is not queried, because the query itself would wake it.
pub fn read_state(sysfs_root: &Path, driver: &dyn NvidiaTuningDriver) -> NvidiaTuningState {
    let availability = availability(sysfs_root);
    if availability != NvidiaAvailability::Ready {
        return NvidiaTuningState {
            availability,
            ..Default::default()
        };
    }
    let [core, memory, power] = driver.read_all().map(Result::ok);
    let state = NvidiaTuningState {
        availability,
        core,
        memory,
        power,
        writable: false,
    };
    if state.has_settings() {
        state
    } else {
        NvidiaTuningState {
            availability: NvidiaAvailability::Unreadable,
            ..Default::default()
        }
    }
}

/// Validate against the driver bounds, write, and confirm by reading back.
/// Returns the read-back value; a mismatch is an error, never a success.
pub fn apply_setting(
    driver: &dyn NvidiaTuningDriver,
    field: NvidiaField,
    value: i32,
) -> Result<i32, ProviderError> {
    let bounds = driver.read(field)?;
    if !bounds.accepts(value) {
        return Err(ProviderError::InvalidRequest(format!(
            "{} {value} вне допустимого диапазона {}..{}",
            field.label(),
            bounds.min,
            bounds.max
        )));
    }
    driver.write(field, value)?;
    let observed = driver.read(field)?.current;
    if observed != value {
        return Err(ProviderError::Conflict(format!(
            "драйвер не подтвердил {}: запрошено {value}, прочитано {observed}",
            field.label()
        )));
    }
    Ok(observed)
}

/// Production driver: NVML, initialised for every call so nothing stays open.
#[derive(Debug, Default, Clone, Copy)]
pub struct NvmlDriver;

fn nvml_error(error: NvmlError) -> ProviderError {
    match error {
        NvmlError::NoPermission => ProviderError::PermissionDenied(error.to_string()),
        NvmlError::NotSupported | NvmlError::FunctionNotFound => {
            ProviderError::Unsupported(error.to_string())
        }
        NvmlError::DriverNotLoaded
        | NvmlError::LibraryNotFound
        | NvmlError::GpuLost
        | NvmlError::Uninitialized => ProviderError::BackendUnavailable(error.to_string()),
        NvmlError::InvalidArg => ProviderError::InvalidRequest(error.to_string()),
        other => ProviderError::Internal(other.to_string()),
    }
}

fn with_device<T>(
    action: impl FnOnce(&mut Device<'_>) -> Result<T, NvmlError>,
) -> Result<T, ProviderError> {
    let nvml = Nvml::init().map_err(nvml_error)?;
    let mut device = nvml.device_by_index(0).map_err(nvml_error)?;
    action(&mut device).map_err(nvml_error)
}

fn milliwatts_to_watts(milliwatts: u32) -> Result<i32, NvmlError> {
    i32::try_from(milliwatts / 1000).map_err(|_| NvmlError::Unknown)
}

fn read_field(device: &Device<'_>, field: NvidiaField) -> Result<NvidiaSetting, NvmlError> {
    match field {
        NvidiaField::CoreOffset | NvidiaField::MemoryOffset => {
            let (clock, current) = if field == NvidiaField::CoreOffset {
                (Clock::Graphics, device.gpc_clock_vf_offset()?)
            } else {
                (Clock::Memory, device.mem_clock_vf_offset()?)
            };
            let range = device.clock_offset(clock, PerformanceState::Zero)?;
            Ok(NvidiaSetting {
                current,
                min: range.min_clock_offset_mhz,
                max: range.max_clock_offset_mhz,
                default: None,
            })
        }
        NvidiaField::PowerLimit => {
            let limits = device.power_management_limit_constraints()?;
            Ok(NvidiaSetting {
                current: milliwatts_to_watts(device.power_management_limit()?)?,
                min: milliwatts_to_watts(limits.min_limit)?,
                max: milliwatts_to_watts(limits.max_limit)?,
                default: device
                    .power_management_limit_default()
                    .ok()
                    .and_then(|value| milliwatts_to_watts(value).ok()),
            })
        }
    }
}

impl NvidiaTuningDriver for NvmlDriver {
    fn read(&self, field: NvidiaField) -> Result<NvidiaSetting, ProviderError> {
        with_device(|device| read_field(device, field))
    }

    fn read_all(&self) -> [Result<NvidiaSetting, ProviderError>; 3] {
        let failed = |error: NvmlError| {
            let message = nvml_error(error).to_string();
            NvidiaField::ALL.map(|_| Err(ProviderError::BackendUnavailable(message.clone())))
        };
        let nvml = match Nvml::init() {
            Ok(nvml) => nvml,
            Err(error) => return failed(error),
        };
        let device = match nvml.device_by_index(0) {
            Ok(device) => device,
            Err(error) => return failed(error),
        };
        NvidiaField::ALL.map(|field| read_field(&device, field).map_err(nvml_error))
    }

    fn write(&self, field: NvidiaField, value: i32) -> Result<(), ProviderError> {
        with_device(|device| match field {
            NvidiaField::CoreOffset => device.set_gpc_clock_vf_offset(value),
            NvidiaField::MemoryOffset => device.set_mem_clock_vf_offset(value),
            NvidiaField::PowerLimit => {
                let watts = u32::try_from(value).map_err(|_| NvmlError::InvalidArg)?;
                device.set_power_management_limit(watts * 1000)
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::os::unix::fs::symlink;
    use std::sync::Mutex;

    use super::*;

    struct FakeDriver {
        settings: Mutex<BTreeMap<NvidiaField, NvidiaSetting>>,
        stuck: bool,
    }

    impl FakeDriver {
        fn new(stuck: bool) -> Self {
            let setting = |min, max| NvidiaSetting {
                current: 0,
                min,
                max,
                default: None,
            };
            Self {
                settings: Mutex::new(BTreeMap::from([
                    (NvidiaField::CoreOffset, setting(-1000, 1000)),
                    (NvidiaField::MemoryOffset, setting(-2000, 6000)),
                ])),
                stuck,
            }
        }
    }

    impl NvidiaTuningDriver for FakeDriver {
        fn read(&self, field: NvidiaField) -> Result<NvidiaSetting, ProviderError> {
            self.settings
                .lock()
                .unwrap()
                .get(&field)
                .copied()
                .ok_or_else(|| ProviderError::Unsupported("нет".into()))
        }

        fn write(&self, field: NvidiaField, value: i32) -> Result<(), ProviderError> {
            if !self.stuck {
                self.settings
                    .lock()
                    .unwrap()
                    .get_mut(&field)
                    .unwrap()
                    .current = value;
            }
            Ok(())
        }
    }

    fn pci_gpu(root: &Path, driver: &str, status: &str) {
        let device = root.join("bus/pci/devices/0000:01:00.0");
        fs::create_dir_all(device.join("power")).unwrap();
        fs::write(device.join("vendor"), "0x10de\n").unwrap();
        fs::write(device.join("class"), "0x030200\n").unwrap();
        fs::write(device.join("power/runtime_status"), format!("{status}\n")).unwrap();
        let drivers = root.join("bus/pci/drivers").join(driver);
        fs::create_dir_all(&drivers).unwrap();
        symlink(&drivers, device.join("driver")).unwrap();
    }

    #[test]
    fn presence_follows_driver_binding_and_runtime_status() {
        let empty = tempfile::tempdir().unwrap();
        assert_eq!(gpu_presence(empty.path()), GpuPresence::Absent);
        let other = tempfile::tempdir().unwrap();
        pci_gpu(other.path(), "vfio-pci", "active");
        assert_eq!(gpu_presence(other.path()), GpuPresence::Absent);
        let asleep = tempfile::tempdir().unwrap();
        pci_gpu(asleep.path(), "nvidia", "suspended");
        assert_eq!(gpu_presence(asleep.path()), GpuPresence::Asleep);
        let awake = tempfile::tempdir().unwrap();
        pci_gpu(awake.path(), "nvidia", "active");
        assert_eq!(gpu_presence(awake.path()), GpuPresence::Awake);
    }

    #[test]
    fn asleep_gpu_is_not_queried_and_missing_settings_stay_hidden() {
        struct Panicking;
        impl NvidiaTuningDriver for Panicking {
            fn read(&self, _: NvidiaField) -> Result<NvidiaSetting, ProviderError> {
                panic!("a sleeping GPU must not be queried");
            }
            fn write(&self, _: NvidiaField, _: i32) -> Result<(), ProviderError> {
                unreachable!()
            }
        }
        let asleep = tempfile::tempdir().unwrap();
        pci_gpu(asleep.path(), "nvidia", "suspended");
        let state = read_state(asleep.path(), &Panicking);
        assert_eq!(state.availability, NvidiaAvailability::Asleep);
        assert!(!state.has_settings());

        let awake = tempfile::tempdir().unwrap();
        pci_gpu(awake.path(), "nvidia", "active");
        let state = read_state(awake.path(), &FakeDriver::new(false));
        assert_eq!(state.availability, NvidiaAvailability::Ready);
        assert!(state.core.is_some() && state.memory.is_some());
        assert!(state.power.is_none(), "unreadable power limit stays hidden");
    }

    #[test]
    fn apply_checks_bounds_and_requires_read_back() {
        let driver = FakeDriver::new(false);
        assert_eq!(
            apply_setting(&driver, NvidiaField::CoreOffset, 150).unwrap(),
            150
        );
        assert!(matches!(
            apply_setting(&driver, NvidiaField::CoreOffset, 1001),
            Err(ProviderError::InvalidRequest(_))
        ));
        assert_eq!(driver.read(NvidiaField::CoreOffset).unwrap().current, 150);
        assert!(matches!(
            apply_setting(&driver, NvidiaField::PowerLimit, 80),
            Err(ProviderError::Unsupported(_))
        ));
        let stuck = FakeDriver::new(true);
        assert!(matches!(
            apply_setting(&stuck, NvidiaField::MemoryOffset, 500),
            Err(ProviderError::Conflict(_))
        ));
    }
}
