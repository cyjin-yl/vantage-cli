use std::path::{Path, PathBuf};

use lctrl_core::{
    ApplyMode, DriverIdentity, DriverPackage, DriverRollbackReport, DriverSignature,
    DriverUpdateReport, InstalledDriver, LctrlError, LocalDriverRollbackRequest,
    LocalDriverUpdateRequest, MAX_LOCAL_UPDATE_DEVICES, MAX_LOCAL_UPDATE_PACKAGE_FILES,
    PresentPnpDevice, Result, is_firmware_class, validate_local_path,
};
use lctrl_hal::LocalUpdateControl;

/// Read-only admission and trust are separate from the two mutating methods.
/// Implementations must enumerate only present, local PnP devices and must not
/// turn a failed install into a generic INF/application setup invocation.
pub trait LocalDriverUpdateApi: Send + Sync {
    fn admit_local_inf(&self, path: &Path) -> Result<PathBuf>;
    fn present_devices(&self) -> Result<Vec<PresentPnpDevice>>;
    fn inspect_package(
        &self,
        inf: &Path,
        device: &PresentPnpDevice,
        hardware_id: &str,
    ) -> Result<DriverPackage>;
    fn verify_package_signature(&self, package: &DriverPackage) -> Result<DriverSignature>;
    fn installed_driver(&self, device_instance_id: &str) -> Result<Option<InstalledDriver>>;
    /// Install only the already-admitted INF. Return reboot need, never reboot.
    fn install_driver(&self, inf: &Path, hardware_id: &str) -> Result<bool>;
    /// Use Windows' documented backup-driver restoration, not INF deletion,
    /// registry rewriting, or guessed driver-store selection.
    fn rollback_driver(&self, device_instance_id: &str) -> Result<bool>;
}

#[derive(Debug)]
pub struct WindowsLocalUpdateController<A> {
    api: A,
}

impl<A> WindowsLocalUpdateController<A> {
    #[must_use]
    pub const fn new(api: A) -> Self {
        Self { api }
    }

    #[must_use]
    pub const fn api(&self) -> &A {
        &self.api
    }
}

impl<A: LocalDriverUpdateApi> LocalUpdateControl for WindowsLocalUpdateController<A> {
    fn apply_local_driver_update(
        &self,
        request: &LocalDriverUpdateRequest,
        mode: ApplyMode,
    ) -> Result<DriverUpdateReport> {
        let inf = self.api.admit_local_inf(request.inf_path())?;
        validate_local_path(&inf, "inf")?;
        let devices = self.api.present_devices()?;
        if devices.len() > MAX_LOCAL_UPDATE_DEVICES {
            return Err(invalid("present-device enumeration exceeds its bound"));
        }
        let mut matching = devices
            .into_iter()
            .filter(|device| device.has_hardware_id(request.hardware_id()));
        let device = matching.next().ok_or_else(|| {
            invalid("requested hardware ID does not bind to a present PnP device")
        })?;
        if matching.next().is_some() {
            return Err(invalid(
                "hardware ID matches multiple present devices; broad multi-device installation is refused",
            ));
        }
        device.validate()?;
        if is_firmware_class(&device.class_guid) {
            return Err(invalid(
                "firmware devices are excluded from local driver updates",
            ));
        }
        let package = self
            .api
            .inspect_package(&inf, &device, request.hardware_id())?;
        validate_package(&package, &inf, &device, request.hardware_id())?;
        let signature = self.api.verify_package_signature(&package)?;
        signature.validate()?;
        if signature != package.signature {
            return Err(invalid(
                "verified signer differs from the inspected driver package",
            ));
        }
        let previous = self.api.installed_driver(&device.device_instance_id)?;
        if mode == ApplyMode::DryRun {
            return Ok(DriverUpdateReport {
                mode,
                device_instance_id: device.device_instance_id,
                previous,
                requested: package.identity,
                actual: None,
                signature,
                reboot_required: None,
            });
        }
        let reboot_required = self.api.install_driver(&inf, request.hardware_id())?;
        let actual = self.api.installed_driver(&device.device_instance_id)?;
        let actual = require_readback(&package.identity, actual)?;
        Ok(DriverUpdateReport {
            mode,
            device_instance_id: device.device_instance_id,
            previous,
            requested: package.identity,
            actual: Some(actual),
            signature,
            reboot_required: Some(reboot_required),
        })
    }

    fn rollback_local_driver(
        &self,
        request: &LocalDriverRollbackRequest,
        mode: ApplyMode,
    ) -> Result<DriverRollbackReport> {
        let devices = self.api.present_devices()?;
        if devices.len() > MAX_LOCAL_UPDATE_DEVICES {
            return Err(invalid("present-device enumeration exceeds its bound"));
        }
        let mut matching = devices.into_iter().filter(|device| {
            device
                .device_instance_id
                .eq_ignore_ascii_case(request.device_instance_id())
        });
        let device = matching
            .next()
            .ok_or_else(|| invalid("rollback device is not present"))?;
        if matching.next().is_some() {
            return Err(invalid("rollback device identity is ambiguous"));
        }
        device.validate()?;
        if is_firmware_class(&device.class_guid)
            || !device
                .class_guid
                .eq_ignore_ascii_case(&request.restore().class_guid)
            || !device.has_hardware_id(&request.restore().matching_device_id)
            || !request
                .expected_current()
                .class_guid
                .eq_ignore_ascii_case(&request.restore().class_guid)
            || !request
                .expected_current()
                .matching_device_id
                .eq_ignore_ascii_case(&request.restore().matching_device_id)
        {
            return Err(invalid(
                "rollback identity does not bind to the present non-firmware device",
            ));
        }
        let previous = require_readback(
            request.expected_current(),
            self.api.installed_driver(request.device_instance_id())?,
        )?;
        if mode == ApplyMode::DryRun {
            return Ok(DriverRollbackReport {
                mode,
                device_instance_id: device.device_instance_id,
                previous,
                requested: request.restore().clone(),
                actual: None,
                backup_identity_verified: false,
                reboot_required: None,
            });
        }
        let reboot_required = self.api.rollback_driver(&device.device_instance_id)?;
        let actual = require_readback(
            request.restore(),
            self.api.installed_driver(&device.device_instance_id)?,
        )?;
        Ok(DriverRollbackReport {
            mode,
            device_instance_id: device.device_instance_id,
            previous,
            requested: request.restore().clone(),
            actual: Some(actual),
            backup_identity_verified: true,
            reboot_required: Some(reboot_required),
        })
    }
}

fn validate_package(
    package: &DriverPackage,
    admitted_inf: &Path,
    device: &PresentPnpDevice,
    hardware_id: &str,
) -> Result<()> {
    if package.inf_path != admitted_inf {
        return Err(invalid(
            "SetupAPI inspection substituted a different INF path",
        ));
    }
    validate_local_path(&package.inf_path, "inf")?;
    validate_local_path(&package.catalog_path, "cat")?;
    package.identity.validate()?;
    package.signature.validate()?;
    if is_firmware_class(&package.identity.class_guid)
        || !package
            .identity
            .class_guid
            .eq_ignore_ascii_case(&device.class_guid)
        || !package
            .identity
            .matching_device_id
            .eq_ignore_ascii_case(hardware_id)
    {
        return Err(invalid(
            "driver package class or hardware ID does not match the present device",
        ));
    }
    if package.dependencies.len() > MAX_LOCAL_UPDATE_PACKAGE_FILES {
        return Err(invalid("driver package dependency count exceeds its bound"));
    }
    for dependency in &package.dependencies {
        // Dependencies can have any driver-package payload extension. The
        // native backend requires regular local files and verifies their own
        // SetupAPI-selected catalogs, including inbox dependencies.
        let extension = dependency
            .path
            .extension()
            .and_then(|value| value.to_str())
            .ok_or_else(|| invalid("driver dependency has no file extension"))?;
        validate_local_path(&dependency.path, extension)?;
        validate_local_path(&dependency.catalog_path, "cat")?;
        dependency.signature.validate()?;
    }
    Ok(())
}

fn require_readback(
    requested: &DriverIdentity,
    actual: Option<InstalledDriver>,
) -> Result<InstalledDriver> {
    if let Some(actual) = actual {
        actual.identity.validate()?;
        if requested.same_driver(&actual.identity) {
            return Ok(actual);
        }
        return Err(LctrlError::VerifyMismatch {
            requested: format!(
                "{} {} ({})",
                requested.provider, requested.driver_version, requested.inf_section
            ),
            actual: format!(
                "{} {} ({})",
                actual.identity.provider,
                actual.identity.driver_version,
                actual.identity.inf_section
            ),
        });
    }
    Err(LctrlError::VerifyMismatch {
        requested: requested.driver_version.to_string(),
        actual: "no installed driver identity".into(),
    })
}

fn invalid(detail: impl Into<String>) -> LctrlError {
    LctrlError::InvalidArgument {
        detail: detail.into(),
    }
}

#[cfg(windows)]
pub use native::NativeLocalDriverUpdateApi;

#[cfg(windows)]
mod native {
    use std::{
        ffi::{OsStr, c_void},
        fs,
        mem::{offset_of, size_of},
        os::windows::ffi::OsStrExt,
        panic::{AssertUnwindSafe, catch_unwind},
        path::{Path, PathBuf},
        ptr,
    };

    use lctrl_core::{
        DriverIdentity, DriverPackage, DriverSignature, DriverVersion, InstalledDriver, LctrlError,
        MAX_LOCAL_UPDATE_DEVICES, MAX_LOCAL_UPDATE_PACKAGE_FILES, PresentPnpDevice, Result,
        SignedDriverFile, validate_hardware_id, validate_local_path,
    };
    use windows_sys::{
        Win32::{
            Devices::DeviceAndDriverInstallation::*,
            Foundation::{
                ERROR_FILE_NOT_FOUND, ERROR_INSUFFICIENT_BUFFER, ERROR_INVALID_DATA,
                ERROR_NO_MORE_ITEMS, GetLastError, INVALID_HANDLE_VALUE, SYSTEMTIME,
            },
            Security::WinTrust::{
                WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_CATALOG_INFO, WINTRUST_DATA,
                WINTRUST_DATA_0, WTD_CACHE_ONLY_URL_RETRIEVAL, WTD_CHOICE_CATALOG,
                WTD_DISABLE_MD2_MD4, WTD_REVOKE_NONE, WTD_STATEACTION_CLOSE,
                WTD_STATEACTION_VERIFY, WTD_UI_NONE, WinVerifyTrust,
            },
            System::{
                Registry::{
                    HKEY, HKEY_LOCAL_MACHINE, KEY_READ, REG_MULTI_SZ, REG_SZ, RegCloseKey,
                    RegOpenKeyExW, RegQueryValueExW,
                },
                Time::FileTimeToSystemTime,
            },
        },
        core::GUID,
    };

    use super::{LocalDriverUpdateApi, invalid};

    const MAX_NATIVE_BUFFER_BYTES: u32 = 64 * 1024;
    const MAX_NATIVE_TEXT: usize = 32768;

    /// Public Windows APIs only. No shell, vendor installer, remote catalog,
    /// firmware flashing, forced ranking override, or reboot API is used.
    #[derive(Clone, Copy, Debug, Default)]
    pub struct NativeLocalDriverUpdateApi;

    impl LocalDriverUpdateApi for NativeLocalDriverUpdateApi {
        fn admit_local_inf(&self, path: &Path) -> Result<PathBuf> {
            canonical_file(path, Some("inf"))
        }

        fn present_devices(&self) -> Result<Vec<PresentPnpDevice>> {
            let set = DeviceSet::present()?;
            let mut devices = Vec::new();
            for index in 0..=MAX_LOCAL_UPDATE_DEVICES {
                let mut data = device_data();
                if unsafe { SetupDiEnumDeviceInfo(set.0, index as u32, &mut data) } == 0 {
                    let error = unsafe { GetLastError() };
                    if error == ERROR_NO_MORE_ITEMS {
                        return Ok(devices);
                    }
                    return Err(crate::map_win_error(error, "SetupDiEnumDeviceInfo"));
                }
                if index == MAX_LOCAL_UPDATE_DEVICES {
                    return Err(invalid("present-device enumeration exceeds its bound"));
                }
                let Some((kind, ids)) = device_property(set.0, &data, SPDRP_HARDWAREID)? else {
                    continue;
                };
                if kind != REG_MULTI_SZ {
                    return Err(invalid("PnP hardware IDs are not a REG_MULTI_SZ value"));
                }
                let hardware_ids = multi_string(&ids)?;
                if hardware_ids.is_empty() {
                    continue;
                }
                devices.push(PresentPnpDevice {
                    device_instance_id: instance_id(set.0, &data)?,
                    hardware_ids,
                    class_guid: format_guid(&data.ClassGuid),
                });
            }
            Err(invalid("present-device enumeration exceeds its bound"))
        }

        fn inspect_package(
            &self,
            inf: &Path,
            device: &PresentPnpDevice,
            hardware_id: &str,
        ) -> Result<DriverPackage> {
            let inf = canonical_file(inf, Some("inf"))?;
            let inf_wide = wide(inf.as_os_str())?;
            let mut class_guid = GUID::default();
            let mut class_name = [0u16; 256];
            let mut required = 0u32;
            if unsafe {
                SetupDiGetINFClassW(
                    inf_wide.as_ptr(),
                    &mut class_guid,
                    class_name.as_mut_ptr(),
                    class_name.len() as u32,
                    &mut required,
                )
            } == 0
            {
                return Err(invalid(format!(
                    "input is not a valid device-driver INF (SetupDiGetINFClassW: 0x{:08x})",
                    unsafe { GetLastError() }
                )));
            }
            if guid_eq(&class_guid, &GUID_DEVCLASS_FIRMWARE) {
                return Err(invalid("firmware driver packages are excluded"));
            }
            let (catalog_path, signature) = inf_signature(&inf)?;
            let (set, mut data) = open_device(&device.device_instance_id)?;
            if !guid_eq(&class_guid, &data.ClassGuid) {
                return Err(invalid(
                    "INF device class differs from the selected present device",
                ));
            }
            let mut parameters = install_parameters(set.0, &data)?;
            parameters.Flags |= DI_ENUMSINGLEINF | DI_QUIETINSTALL;
            parameters.FlagsEx |=
                DI_FLAGSEX_ALLOWEXCLUDEDDRVS | DI_FLAGSEX_EXCLUDE_OLD_INET_DRIVERS;
            parameters.FlagsEx &= !(DI_FLAGSEX_DRIVERLIST_FROM_URL
                | DI_FLAGSEX_INET_DRIVER
                | DI_FLAGSEX_RECURSIVESEARCH);
            set_fixed_wide(&mut parameters.DriverPath, &inf_wide)?;
            set_install_parameters(set.0, &data, &parameters)?;
            if unsafe { SetupDiBuildDriverInfoList(set.0, &mut data, SPDIT_COMPATDRIVER) } == 0 {
                return Err(last_error(
                    "SetupDiBuildDriverInfoList local compatible driver",
                ));
            }
            let mut selected: Option<(SP_DRVINFO_DATA_V2_W, DriverIdentity)> = None;
            for index in 0..=MAX_LOCAL_UPDATE_PACKAGE_FILES {
                let mut node = SP_DRVINFO_DATA_V2_W::default();
                node.cbSize = size_of::<SP_DRVINFO_DATA_V2_W>() as u32;
                if unsafe {
                    SetupDiEnumDriverInfoW(
                        set.0,
                        &data,
                        SPDIT_COMPATDRIVER,
                        index as u32,
                        &mut node,
                    )
                } == 0
                {
                    let error = unsafe { GetLastError() };
                    if error == ERROR_NO_MORE_ITEMS {
                        break;
                    }
                    return Err(crate::map_win_error(error, "SetupDiEnumDriverInfoW"));
                }
                if index == MAX_LOCAL_UPDATE_PACKAGE_FILES {
                    return Err(invalid(
                        "compatible driver-node enumeration exceeds its bound",
                    ));
                }
                let detail = driver_detail(set.0, &data, &node)?;
                if !detail.hardware_id.eq_ignore_ascii_case(hardware_id)
                    || canonical_file(&detail.inf_path, Some("inf"))? != inf
                {
                    continue;
                }
                let identity = DriverIdentity {
                    provider: fixed_text(&node.ProviderName)?,
                    description: fixed_text(&node.Description)?,
                    class_guid: format_guid(&class_guid),
                    driver_version: DriverVersion::from_raw(node.DriverVersion),
                    driver_date: driver_date(node.DriverDate)?,
                    matching_device_id: detail.hardware_id,
                    inf_section: detail.section,
                };
                identity.validate()?;
                if let Some((_, previous)) = &selected {
                    if !previous.same_driver(&identity) {
                        return Err(invalid(
                            "local INF has multiple distinct matching driver identities",
                        ));
                    }
                } else {
                    selected = Some((node, identity));
                }
            }
            let (mut node, identity) = selected.ok_or_else(|| {
                invalid("local INF has no driver node matching the requested present-device hardware ID")
            })?;
            if unsafe { SetupDiSetSelectedDriverW(set.0, &mut data, &mut node) } == 0 {
                return Err(last_error("SetupDiSetSelectedDriverW inspection"));
            }

            // DI_NOVCP plus a caller-owned queue makes this a queue-building
            // operation. The queue is scanned and discarded, never committed.
            // SetupAPI, rather than a second INF parser, resolves CopyFiles,
            // Include/Needs and decorated sections for the selected driver.
            let queue = FileQueue::new()?;
            let mut parameters = install_parameters(set.0, &data)?;
            parameters.Flags |= DI_NOVCP | DI_QUIETINSTALL;
            parameters.Flags &= !DI_NOFILECOPY;
            parameters.FileQueue = queue.0;
            parameters.InstallMsgHandler = None;
            parameters.InstallMsgHandlerContext = ptr::null_mut();
            set_install_parameters(set.0, &data, &parameters)?;
            if unsafe { SetupDiInstallDriverFiles(set.0, &data) } == 0 {
                return Err(last_error("SetupDiInstallDriverFiles admission queue"));
            }
            let dependencies = scan_dependencies(&queue)?;
            Ok(DriverPackage {
                inf_path: inf,
                catalog_path,
                signature,
                dependencies,
                identity,
            })
        }

        fn verify_package_signature(&self, package: &DriverPackage) -> Result<DriverSignature> {
            verify_catalog_member(&package.inf_path, &package.catalog_path)?;
            for dependency in &package.dependencies {
                verify_catalog_member(&dependency.path, &dependency.catalog_path)?;
            }
            Ok(package.signature.clone())
        }

        fn installed_driver(&self, device_instance_id: &str) -> Result<Option<InstalledDriver>> {
            let (set, data) = open_device(device_instance_id)?;
            let Some((kind, value)) = device_property(set.0, &data, SPDRP_DRIVER)? else {
                return Ok(None);
            };
            if kind != REG_SZ {
                return Err(invalid("installed driver key is not a REG_SZ value"));
            }
            let driver_key = fixed_text(&value)?;
            if driver_key.is_empty() {
                return Ok(None);
            }
            let key = RegistryKey::open(&format!(
                "SYSTEM\\CurrentControlSet\\Control\\Class\\{driver_key}"
            ))?;
            let identity = DriverIdentity {
                provider: key.string("ProviderName")?,
                description: key.string("DriverDesc")?,
                class_guid: format_guid(&data.ClassGuid),
                driver_version: DriverVersion::parse(&key.string("DriverVersion")?)?,
                driver_date: key.string("DriverDate")?,
                matching_device_id: key.string("MatchingDeviceId")?,
                inf_section: key.string("InfSection")?,
            };
            identity.validate()?;
            Ok(Some(InstalledDriver {
                identity,
                published_inf: key.string("InfPath")?,
                driver_key,
            }))
        }

        fn install_driver(&self, inf: &Path, hardware_id: &str) -> Result<bool> {
            validate_hardware_id(hardware_id)?;
            let inf = canonical_file(inf, Some("inf"))?;
            let inf = wide(inf.as_os_str())?;
            let hardware_id = wide(OsStr::new(hardware_id))?;
            let mut reboot = 0;
            // No FORCE and no broad DiInstallDriverW fallback. Supplying the
            // reboot pointer prevents newdev from initiating/prompting reboot.
            if unsafe {
                UpdateDriverForPlugAndPlayDevicesW(
                    ptr::null_mut(),
                    hardware_id.as_ptr(),
                    inf.as_ptr(),
                    INSTALLFLAG_NONINTERACTIVE,
                    &mut reboot,
                )
            } == 0
            {
                let error = unsafe { GetLastError() };
                if error == ERROR_NO_MORE_ITEMS {
                    return Err(invalid(
                        "Windows rejected the package as not a better driver match; forcing is not supported",
                    ));
                }
                return Err(crate::map_win_error(
                    error,
                    "UpdateDriverForPlugAndPlayDevicesW",
                ));
            }
            Ok(reboot != 0)
        }

        fn rollback_driver(&self, device_instance_id: &str) -> Result<bool> {
            let (set, data) = open_device(device_instance_id)?;
            let mut reboot = 0;
            if unsafe {
                DiRollbackDriver(
                    set.0,
                    &data,
                    ptr::null_mut(),
                    ROLLBACK_FLAG_NO_UI,
                    &mut reboot,
                )
            } == 0
            {
                let error = unsafe { GetLastError() };
                if error == ERROR_NO_MORE_ITEMS {
                    return Err(LctrlError::Unsupported {
                        feature: "update.local_driver.rollback.no_previous_driver_store_point"
                            .into(),
                    });
                }
                return Err(crate::map_win_error(error, "DiRollbackDriver"));
            }
            Ok(reboot != 0)
        }
    }

    struct DeviceSet(HDEVINFO);

    impl DeviceSet {
        fn present() -> Result<Self> {
            let handle = unsafe {
                SetupDiGetClassDevsW(
                    ptr::null(),
                    ptr::null(),
                    ptr::null_mut(),
                    DIGCF_PRESENT | DIGCF_ALLCLASSES,
                )
            };
            if handle == -1 {
                Err(last_error("SetupDiGetClassDevsW present local devices"))
            } else {
                Ok(Self(handle))
            }
        }
    }

    impl Drop for DeviceSet {
        fn drop(&mut self) {
            unsafe { SetupDiDestroyDeviceInfoList(self.0) };
        }
    }

    fn open_device(instance: &str) -> Result<(DeviceSet, SP_DEVINFO_DATA)> {
        if instance.is_empty()
            || instance.trim() != instance
            || instance.encode_utf16().count() > MAX_DEVICE_ID_LEN as usize
            || instance.chars().any(char::is_control)
        {
            return Err(invalid(
                "PnP device instance ID is empty, too long, or contains control text",
            ));
        }
        let handle = unsafe { SetupDiCreateDeviceInfoList(ptr::null(), ptr::null_mut()) };
        if handle == -1 {
            return Err(last_error("SetupDiCreateDeviceInfoList"));
        }
        let set = DeviceSet(handle);
        let mut data = device_data();
        let instance = wide(OsStr::new(instance))?;
        if unsafe {
            SetupDiOpenDeviceInfoW(set.0, instance.as_ptr(), ptr::null_mut(), 0, &mut data)
        } == 0
        {
            return Err(last_error("SetupDiOpenDeviceInfoW exact local instance"));
        }
        Ok((set, data))
    }

    fn device_data() -> SP_DEVINFO_DATA {
        SP_DEVINFO_DATA {
            cbSize: size_of::<SP_DEVINFO_DATA>() as u32,
            ..SP_DEVINFO_DATA::default()
        }
    }

    fn instance_id(set: HDEVINFO, data: &SP_DEVINFO_DATA) -> Result<String> {
        let mut buffer = [0u16; MAX_DEVICE_ID_LEN as usize + 1];
        let mut required = 0;
        if unsafe {
            SetupDiGetDeviceInstanceIdW(
                set,
                data,
                buffer.as_mut_ptr(),
                buffer.len() as u32,
                &mut required,
            )
        } == 0
        {
            return Err(last_error("SetupDiGetDeviceInstanceIdW"));
        }
        fixed_text(&buffer)
    }

    fn device_property(
        set: HDEVINFO,
        data: &SP_DEVINFO_DATA,
        property: SETUP_DI_REGISTRY_PROPERTY,
    ) -> Result<Option<(u32, Vec<u16>)>> {
        let mut kind = 0;
        let mut required = 0;
        let status = unsafe {
            SetupDiGetDeviceRegistryPropertyW(
                set,
                data,
                property,
                &mut kind,
                ptr::null_mut(),
                0,
                &mut required,
            )
        };
        if status == 0 {
            let error = unsafe { GetLastError() };
            if error == ERROR_INVALID_DATA || error == ERROR_FILE_NOT_FOUND {
                return Ok(None);
            }
            if error != ERROR_INSUFFICIENT_BUFFER {
                return Err(crate::map_win_error(
                    error,
                    "SetupDiGetDeviceRegistryPropertyW size",
                ));
            }
        }
        if required == 0 || required > MAX_NATIVE_BUFFER_BYTES || required % 2 != 0 {
            return Err(invalid(
                "PnP registry property has an invalid bounded UTF-16 length",
            ));
        }
        let mut buffer = vec![0u16; required as usize / 2];
        if unsafe {
            SetupDiGetDeviceRegistryPropertyW(
                set,
                data,
                property,
                &mut kind,
                buffer.as_mut_ptr().cast(),
                required,
                &mut required,
            )
        } == 0
        {
            return Err(last_error("SetupDiGetDeviceRegistryPropertyW value"));
        }
        if required as usize > buffer.len() * 2 || required % 2 != 0 {
            return Err(invalid("PnP registry property changed to an invalid size"));
        }
        buffer.truncate(required as usize / 2);
        Ok(Some((kind, buffer)))
    }

    fn multi_string(buffer: &[u16]) -> Result<Vec<String>> {
        if buffer.len() < 2 || buffer[buffer.len() - 2..] != [0, 0] {
            return Err(invalid("PnP hardware-ID list is not double-NUL terminated"));
        }
        let mut values = Vec::new();
        for value in buffer.split(|unit| *unit == 0) {
            if value.is_empty() {
                break;
            }
            values.push(
                String::from_utf16(value).map_err(|_| invalid("PnP ID is not valid UTF-16"))?,
            );
            if values.len() > 64 {
                return Err(invalid("PnP hardware-ID count exceeds its bound"));
            }
        }
        Ok(values)
    }

    fn install_parameters(set: HDEVINFO, data: &SP_DEVINFO_DATA) -> Result<SP_DEVINSTALL_PARAMS_W> {
        let mut parameters = SP_DEVINSTALL_PARAMS_W::default();
        parameters.cbSize = size_of::<SP_DEVINSTALL_PARAMS_W>() as u32;
        if unsafe { SetupDiGetDeviceInstallParamsW(set, data, &mut parameters) } == 0 {
            return Err(last_error("SetupDiGetDeviceInstallParamsW"));
        }
        Ok(parameters)
    }

    fn set_install_parameters(
        set: HDEVINFO,
        data: &SP_DEVINFO_DATA,
        parameters: &SP_DEVINSTALL_PARAMS_W,
    ) -> Result<()> {
        if unsafe { SetupDiSetDeviceInstallParamsW(set, data, parameters) } == 0 {
            return Err(last_error("SetupDiSetDeviceInstallParamsW"));
        }
        Ok(())
    }

    struct DriverDetail {
        inf_path: PathBuf,
        section: String,
        hardware_id: String,
    }

    fn driver_detail(
        set: HDEVINFO,
        data: &SP_DEVINFO_DATA,
        node: &SP_DRVINFO_DATA_V2_W,
    ) -> Result<DriverDetail> {
        let mut required = 0;
        unsafe { SetupDiGetDriverInfoDetailW(set, data, node, ptr::null_mut(), 0, &mut required) };
        if required < size_of::<SP_DRVINFO_DETAIL_DATA_W>() as u32
            || required > MAX_NATIVE_BUFFER_BYTES
        {
            return Err(invalid(
                "SetupAPI driver detail has an invalid bounded length",
            ));
        }
        // u64 storage supplies alignment for both native and x86 packed SDK
        // layouts; variable HardwareID data remains inside the returned size.
        let mut storage = vec![0u64; (required as usize + 7) / 8];
        let detail = storage.as_mut_ptr().cast::<SP_DRVINFO_DETAIL_DATA_W>();
        unsafe {
            ptr::addr_of_mut!((*detail).cbSize)
                .write_unaligned(size_of::<SP_DRVINFO_DETAIL_DATA_W>() as u32)
        };
        if unsafe { SetupDiGetDriverInfoDetailW(set, data, node, detail, required, &mut required) }
            == 0
        {
            return Err(last_error("SetupDiGetDriverInfoDetailW"));
        }
        let offset = offset_of!(SP_DRVINFO_DETAIL_DATA_W, HardwareID);
        if required as usize <= offset || required as usize > storage.len() * 8 {
            return Err(invalid(
                "SetupAPI driver hardware-ID detail is out of bounds",
            ));
        }
        let compat_offset = unsafe { ptr::addr_of!((*detail).CompatIDsOffset).read_unaligned() };
        if compat_offset <= 1 {
            return Err(invalid(
                "driver node has no exact hardware ID; compatible-ID-only updates are refused",
            ));
        }
        let hardware = unsafe {
            std::slice::from_raw_parts(
                storage.as_ptr().cast::<u8>().add(offset).cast::<u16>(),
                (required as usize - offset) / 2,
            )
        };
        let inf_name = unsafe { ptr::addr_of!((*detail).InfFileName).read_unaligned() };
        let section = unsafe { ptr::addr_of!((*detail).SectionName).read_unaligned() };
        Ok(DriverDetail {
            inf_path: PathBuf::from(fixed_text(&inf_name)?),
            section: fixed_text(&section)?,
            hardware_id: fixed_text(hardware)?,
        })
    }

    fn inf_signature(inf: &Path) -> Result<(PathBuf, DriverSignature)> {
        let inf_wide = wide(inf.as_os_str())?;
        let mut info = SP_INF_SIGNER_INFO_V2_W::default();
        info.cbSize = size_of::<SP_INF_SIGNER_INFO_V2_W>() as u32;
        if unsafe { SetupVerifyInfFileW(inf_wide.as_ptr(), ptr::null(), &mut info) } == 0 {
            return Err(invalid(format!(
                "INF catalog signature was rejected (SetupVerifyInfFileW: 0x{:08x})",
                unsafe { GetLastError() }
            )));
        }
        let catalog = PathBuf::from(fixed_text(&info.CatalogFile)?);
        let catalog = if catalog.is_absolute() {
            catalog
        } else {
            inf.parent()
                .ok_or_else(|| invalid("INF has no parent directory"))?
                .join(catalog)
        };
        let signature = DriverSignature {
            signer: fixed_text(&info.DigitalSigner)?,
            signer_version: fixed_text(&info.DigitalSignerVersion)?,
        };
        signature.validate()?;
        Ok((canonical_file(&catalog, Some("cat"))?, signature))
    }

    struct FileQueue(*mut c_void);

    impl FileQueue {
        fn new() -> Result<Self> {
            let handle = unsafe { SetupOpenFileQueue() };
            if handle.is_null() || handle as usize == usize::MAX {
                Err(last_error("SetupOpenFileQueue"))
            } else {
                Ok(Self(handle))
            }
        }
    }

    impl Drop for FileQueue {
        fn drop(&mut self) {
            unsafe { SetupCloseFileQueue(self.0) };
        }
    }

    #[derive(Default)]
    struct QueueScan {
        files: Vec<SignedDriverFile>,
        error: Option<String>,
    }

    fn scan_dependencies(queue: &FileQueue) -> Result<Vec<SignedDriverFile>> {
        let mut scan = QueueScan::default();
        let mut result = 0;
        let status = unsafe {
            SetupScanFileQueueW(
                queue.0,
                SPQ_SCAN_USE_CALLBACKEX | SPQ_SCAN_USE_CALLBACK_SIGNERINFO,
                ptr::null_mut(),
                Some(queue_callback),
                (&mut scan as *mut QueueScan).cast(),
                &mut result,
            )
        };
        if let Some(error) = scan.error {
            return Err(invalid(error));
        }
        if status == 0 {
            return Err(invalid(format!(
                "driver dependency signature scan failed (0x{:08x})",
                unsafe { GetLastError() }
            )));
        }
        Ok(scan.files)
    }

    unsafe extern "system" fn queue_callback(
        context: *const c_void,
        notification: u32,
        param1: usize,
        _param2: usize,
    ) -> u32 {
        if context.is_null() || param1 == 0 {
            return 1;
        }
        let scan = unsafe { &mut *context.cast_mut().cast::<QueueScan>() };
        let result = catch_unwind(AssertUnwindSafe(|| -> Result<()> {
            if notification != SPFILENOTIFY_QUEUESCAN_SIGNERINFO {
                return Err(invalid(
                    "SetupAPI dependency scan did not return signer metadata",
                ));
            }
            let paths = unsafe { (param1 as *const FILEPATHS_SIGNERINFO_W).read_unaligned() };
            if paths.Win32Error != 0 {
                return Err(invalid(format!(
                    "driver dependency signature was rejected (0x{:08x})",
                    paths.Win32Error
                )));
            }
            let source = unsafe { pointer_text(paths.Source)? };
            let catalog = unsafe { pointer_text(paths.CatalogFile)? };
            let signature = DriverSignature {
                signer: unsafe { pointer_text(paths.DigitalSigner)? },
                signer_version: if paths.Version.is_null() {
                    String::new()
                } else {
                    unsafe { pointer_text(paths.Version)? }
                },
            };
            signature.validate()?;
            let file = SignedDriverFile {
                path: canonical_file(Path::new(&source), None)?,
                catalog_path: canonical_file(Path::new(&catalog), Some("cat"))?,
                signature,
            };
            if !scan.files.contains(&file) {
                if scan.files.len() == MAX_LOCAL_UPDATE_PACKAGE_FILES {
                    return Err(invalid("driver dependency count exceeds its bound"));
                }
                scan.files.push(file);
            }
            Ok(())
        }));
        match result {
            Ok(Ok(())) => 0,
            Ok(Err(error)) => {
                scan.error = Some(error.to_string());
                1
            }
            Err(_) => {
                scan.error = Some("driver dependency callback failed".into());
                1
            }
        }
    }

    fn verify_catalog_member(member: &Path, catalog: &Path) -> Result<()> {
        let member = canonical_file(member, None)?;
        let catalog = canonical_file(catalog, Some("cat"))?;
        let member = wide(member.as_os_str())?;
        let catalog = wide(catalog.as_os_str())?;
        let mut catalog_info = WINTRUST_CATALOG_INFO {
            cbStruct: size_of::<WINTRUST_CATALOG_INFO>() as u32,
            pcwszCatalogFilePath: catalog.as_ptr(),
            pcwszMemberFilePath: member.as_ptr(),
            ..WINTRUST_CATALOG_INFO::default()
        };
        let mut data = WINTRUST_DATA {
            cbStruct: size_of::<WINTRUST_DATA>() as u32,
            dwUIChoice: WTD_UI_NONE,
            fdwRevocationChecks: WTD_REVOKE_NONE,
            dwUnionChoice: WTD_CHOICE_CATALOG,
            Anonymous: WINTRUST_DATA_0 {
                pCatalog: &mut catalog_info,
            },
            dwStateAction: WTD_STATEACTION_VERIFY,
            // This local-only controller never retrieves certificates/CRLs
            // from URLs. Offline trust availability is not fabricated.
            dwProvFlags: WTD_CACHE_ONLY_URL_RETRIEVAL | WTD_DISABLE_MD2_MD4,
            ..WINTRUST_DATA::default()
        };
        let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
        let status = unsafe {
            WinVerifyTrust(
                INVALID_HANDLE_VALUE,
                &mut action,
                (&mut data as *mut WINTRUST_DATA).cast(),
            )
        };
        // CLOSE is mandatory even on trust failure. Preserve the verification
        // status; cleanup must not turn a rejected signature into success.
        data.dwStateAction = WTD_STATEACTION_CLOSE;
        unsafe {
            WinVerifyTrust(
                INVALID_HANDLE_VALUE,
                &mut action,
                (&mut data as *mut WINTRUST_DATA).cast(),
            )
        };
        if status != 0 {
            return Err(invalid(format!(
                "driver package Authenticode trust was rejected (0x{:08x})",
                status as u32
            )));
        }
        Ok(())
    }

    struct RegistryKey(HKEY);

    impl RegistryKey {
        fn open(path: &str) -> Result<Self> {
            let path = wide(OsStr::new(path))?;
            let mut key = ptr::null_mut();
            let status =
                unsafe { RegOpenKeyExW(HKEY_LOCAL_MACHINE, path.as_ptr(), 0, KEY_READ, &mut key) };
            if status != 0 {
                return Err(crate::map_win_error(
                    status,
                    "RegOpenKeyExW installed driver",
                ));
            }
            Ok(Self(key))
        }

        fn string(&self, name: &str) -> Result<String> {
            let name = wide(OsStr::new(name))?;
            let mut kind = 0;
            let mut length = 0;
            let status = unsafe {
                RegQueryValueExW(
                    self.0,
                    name.as_ptr(),
                    ptr::null(),
                    &mut kind,
                    ptr::null_mut(),
                    &mut length,
                )
            };
            if status != 0 {
                return Err(crate::map_win_error(
                    status,
                    "RegQueryValueExW driver identity size",
                ));
            }
            if kind != REG_SZ || length < 2 || length % 2 != 0 || length > MAX_NATIVE_BUFFER_BYTES {
                return Err(invalid(
                    "installed driver identity is not a bounded REG_SZ value",
                ));
            }
            let mut buffer = vec![0u16; length as usize / 2];
            let status = unsafe {
                RegQueryValueExW(
                    self.0,
                    name.as_ptr(),
                    ptr::null(),
                    &mut kind,
                    buffer.as_mut_ptr().cast(),
                    &mut length,
                )
            };
            if status != 0 {
                return Err(crate::map_win_error(
                    status,
                    "RegQueryValueExW driver identity",
                ));
            }
            if kind != REG_SZ || length as usize > buffer.len() * 2 || length % 2 != 0 {
                return Err(invalid(
                    "installed driver registry identity changed type or length",
                ));
            }
            buffer.truncate(length as usize / 2);
            fixed_text(&buffer)
        }
    }

    impl Drop for RegistryKey {
        fn drop(&mut self) {
            unsafe { RegCloseKey(self.0) };
        }
    }

    fn canonical_file(path: &Path, extension: Option<&str>) -> Result<PathBuf> {
        let actual_extension = path.extension().and_then(|value| value.to_str());
        let extension = extension
            .or(actual_extension)
            .ok_or_else(|| invalid("driver file has no extension"))?;
        validate_local_path(path, extension)?;
        let canonical = fs::canonicalize(path)?;
        validate_local_path(&canonical, extension)?;
        if !fs::metadata(&canonical)?.is_file() {
            return Err(invalid("driver package input is not a regular local file"));
        }
        Ok(canonical)
    }

    fn wide(text: &OsStr) -> Result<Vec<u16>> {
        let mut value: Vec<u16> = text.encode_wide().collect();
        if value.contains(&0) || value.len() >= MAX_NATIVE_TEXT {
            return Err(invalid(
                "Windows update argument contains NUL or exceeds its bound",
            ));
        }
        value.push(0);
        Ok(value)
    }

    fn set_fixed_wide<const N: usize>(target: &mut [u16; N], value: &[u16]) -> Result<()> {
        if value.len() > N {
            return Err(invalid(
                "local INF path exceeds the SetupAPI DriverPath limit",
            ));
        }
        target.fill(0);
        target[..value.len()].copy_from_slice(value);
        Ok(())
    }

    fn fixed_text(value: &[u16]) -> Result<String> {
        let end = value
            .iter()
            .position(|unit| *unit == 0)
            .ok_or_else(|| invalid("Windows update text is not NUL terminated"))?;
        String::from_utf16(&value[..end])
            .map_err(|_| invalid("Windows update text is not valid UTF-16"))
    }

    unsafe fn pointer_text(value: *const u16) -> Result<String> {
        if value.is_null() {
            return Err(invalid("SetupAPI returned null signature/path text"));
        }
        for length in 0..MAX_NATIVE_TEXT {
            if unsafe { *value.add(length) } == 0 {
                return String::from_utf16(unsafe { std::slice::from_raw_parts(value, length) })
                    .map_err(|_| invalid("SetupAPI returned invalid UTF-16 signature/path text"));
            }
        }
        Err(invalid("SetupAPI signature/path text exceeds its bound"))
    }

    fn driver_date(date: windows_sys::Win32::Foundation::FILETIME) -> Result<String> {
        let mut calendar = SYSTEMTIME::default();
        if unsafe { FileTimeToSystemTime(&date, &mut calendar) } == 0 {
            return Err(last_error("FileTimeToSystemTime driver date"));
        }
        Ok(format!(
            "{}-{}-{}",
            calendar.wMonth, calendar.wDay, calendar.wYear
        ))
    }

    fn format_guid(guid: &GUID) -> String {
        format!(
            "{{{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}}}",
            guid.data1,
            guid.data2,
            guid.data3,
            guid.data4[0],
            guid.data4[1],
            guid.data4[2],
            guid.data4[3],
            guid.data4[4],
            guid.data4[5],
            guid.data4[6],
            guid.data4[7]
        )
    }

    fn guid_eq(left: &GUID, right: &GUID) -> bool {
        left.data1 == right.data1
            && left.data2 == right.data2
            && left.data3 == right.data3
            && left.data4 == right.data4
    }

    fn last_error(context: &str) -> LctrlError {
        let error = unsafe { GetLastError() };
        crate::map_win_error(error, context)
    }
}
