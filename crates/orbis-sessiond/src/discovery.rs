//! Read-only discovery единственной системной батареи через UPower.

use orbis_providers::error::ProviderError;
use zbus::proxy::CacheProperties;

/// UPower D-Bus service name.
const UPOWER_BUS_NAME: &str = "org.freedesktop.UPower";
/// UPower root object path.
const UPOWER_ROOT_PATH: &str = "/org/freedesktop/UPower";
/// UPower root interface.
const UPOWER_ROOT_INTERFACE: &str = "org.freedesktop.UPower";
/// UPower Device interface.
const UPOWER_DEVICE_INTERFACE: &str = "org.freedesktop.UPower.Device";
/// UPower `Device::Type` value for Battery (UPower spec).
const UPOWER_DEVICE_TYPE_BATTERY: u32 = 2;

/// Root UPower proxy: только `EnumerateDevices`.
#[zbus::proxy(interface = "org.freedesktop.UPower")]
trait UPowerRoot {
    /// Перечислить устройства питания.
    fn enumerate_devices(&self) -> zbus::Result<Vec<zbus::zvariant::OwnedObjectPath>>;
}

/// Device UPower proxy: только `Type` и `PowerSupply`.
#[zbus::proxy(interface = "org.freedesktop.UPower.Device")]
trait UPowerDeviceInfo {
    /// Тип источника питания.
    #[zbus(property)]
    fn type_(&self) -> zbus::Result<u32>;

    /// Является ли устройство системным источником питания.
    #[zbus(property)]
    fn power_supply(&self) -> zbus::Result<bool>;
}

/// Найти единственную системную батарею (Type=Battery, PowerSupply=true).
///
/// - Connection передаётся caller'ом; helper bus не открывает;
/// - service/path задаются через production constants (единый источник);
/// - для обоих proxy используется `CacheProperties::No` (прямые отдельные Get,
///   без GetAll);
/// - сначала читается `Type`, `PowerSupply` — только для Battery;
/// - один кандидат → его путь; кандидатов нет или несколько → `Unsupported`;
///   первый кандидат молча не выбирается;
/// - D-Bus/proxy/property errors → `ProviderError::Dbus`.
pub async fn discover_battery_object_path(
    connection: &zbus::Connection,
) -> Result<zbus::zvariant::OwnedObjectPath, ProviderError> {
    let root = UPowerRootProxy::builder(connection)
        .destination(UPOWER_BUS_NAME)
        .map_err(|e| ProviderError::Dbus(e.to_string()))?
        .path(UPOWER_ROOT_PATH)
        .map_err(|e| ProviderError::Dbus(e.to_string()))?
        .cache_properties(CacheProperties::No)
        .build()
        .await
        .map_err(|e| ProviderError::Dbus(e.to_string()))?;
    let devices = root
        .enumerate_devices()
        .await
        .map_err(|e| ProviderError::Dbus(e.to_string()))?;

    let mut candidates = Vec::new();
    for path in devices {
        let device = UPowerDeviceInfoProxy::builder(connection)
            .destination(UPOWER_BUS_NAME)
            .map_err(|e| ProviderError::Dbus(e.to_string()))?
            .path(path.clone())
            .map_err(|e| ProviderError::Dbus(e.to_string()))?
            .cache_properties(CacheProperties::No)
            .build()
            .await
            .map_err(|e| ProviderError::Dbus(e.to_string()))?;

        let kind = device
            .type_()
            .await
            .map_err(|e| ProviderError::Dbus(e.to_string()))?;
        if kind != UPOWER_DEVICE_TYPE_BATTERY {
            continue;
        }
        let power_supply = device
            .power_supply()
            .await
            .map_err(|e| ProviderError::Dbus(e.to_string()))?;
        if power_supply {
            candidates.push(path);
        }
    }

    match candidates.as_slice() {
        [single] => Ok(single.clone()),
        [] => Err(ProviderError::Unsupported(format!(
            "{UPOWER_ROOT_INTERFACE}: системная батарея не найдена"
        ))),
        _ => Err(ProviderError::Unsupported(format!(
            "{UPOWER_DEVICE_INTERFACE}: несколько системных батарей; автоматический выбор пока не поддерживается"
        ))),
    }
}
