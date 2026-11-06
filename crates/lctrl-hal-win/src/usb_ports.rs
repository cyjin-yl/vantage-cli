//! Read-only USB port inventory over SetupAPI and the USB hub IOCTLs.
//!
//! Sources, their live values and the rejected alternatives are recorded in
//! `docs/evidence/re-usb-ports-20261105.md`:
//!
//! * hubs: every present `GUID_DEVINTERFACE_USB_HUB` device interface;
//! * hub shape: `IOCTL_USB_GET_NODE_INFORMATION`;
//! * per-port state and speed: `IOCTL_USB_GET_NODE_CONNECTION_INFORMATION_EX`;
//! * attached device: `IOCTL_USB_GET_NODE_CONNECTION_DRIVERKEY_NAME` joined
//!   against the hub's child devnodes (`DEVPKEY_Device_Driver`), so the
//!   identity is exact rather than address-guessed;
//! * declared device power: `IOCTL_USB_GET_DESCRIPTOR_FROM_NODE_CONNECTION`.
//!
//! `IOCTL_USB_GET_NODE_CONNECTION_INFORMATION_EX_V2` answers
//! `ERROR_INVALID_PARAMETER` (87) on this SKU for every input length, so no
//! SuperSpeedPlus/USB4 grade is ever claimed from it, and per-port power
//! delivery is left empty because no channel on this platform reports one.

use std::ffi::c_void;
use std::io;
use std::mem::size_of;

use lctrl_core::{
    Availability, CapabilitySet, LctrlError, PowerDeliveryReport, Result, UsbController,
    UsbControllerKind, UsbHub, UsbPdController, UsbPort, UsbPortDevice, UsbPortInventory,
};
use lctrl_hal::UsbPortInventoryControl;
use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
    CM_Get_Child, CM_Get_Device_IDW, CM_Get_DevNode_PropertyW, CM_Get_Sibling, CM_Locate_DevNodeW,
    CR_SUCCESS, DIGCF_DEVICEINTERFACE, DIGCF_PRESENT, GUID_DEVCLASS_UCM, GUID_DEVCLASS_USB,
    HDEVINFO, MAX_DEVICE_ID_LEN, SP_DEVICE_INTERFACE_DATA, SP_DEVICE_INTERFACE_DETAIL_DATA_W,
    SP_DEVINFO_DATA, SETUP_DI_GET_CLASS_DEVS_FLAGS, SetupDiDestroyDeviceInfoList,
    SetupDiEnumDeviceInfo, SetupDiEnumDeviceInterfaces, SetupDiGetClassDevsW,
    SetupDiGetDeviceInterfaceDetailW, SetupDiGetDevicePropertyW,
};
use windows_sys::Win32::Devices::Properties::{
    DEVPKEY_Device_BusReportedDeviceDesc, DEVPKEY_Device_DeviceDesc, DEVPKEY_Device_Driver,
    DEVPKEY_Device_FirmwareVersion, DEVPKEY_Device_InstanceId, DEVPKEY_Device_Parent,
    DEVPKEY_Device_Service, DEVPROP_TYPE_STRING, DEVPROP_TYPE_STRING_INDIRECT,
};
use windows_sys::Win32::Devices::Usb::{
    GUID_DEVINTERFACE_USB_HUB, IOCTL_USB_GET_DESCRIPTOR_FROM_NODE_CONNECTION,
    IOCTL_USB_GET_NODE_CONNECTION_DRIVERKEY_NAME, IOCTL_USB_GET_NODE_CONNECTION_INFORMATION_EX,
    IOCTL_USB_GET_NODE_INFORMATION,
};
use windows_sys::Win32::Foundation::{CloseHandle, DEVPROPKEY, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows_sys::Win32::System::IO::DeviceIoControl;

use crate::usb_port_decode::{
    DeviceConfigurationFacts, decode_config_descriptor, decode_hub_node_information,
    decode_port_connection_information, format_usb_revision, max_power_ma,
};

/// Output capacity for the hub IOCTLs; the driver answers with the packed
/// structures the decoders expect (35-57 bytes on the target).
const IOCTL_OUTPUT_BYTES: usize = 4096;
/// `USB_GET_DESCRIPTOR_FROM_NODE_CONNECTION` input: header + `wLength` (255).
const DESCRIPTOR_REQUEST_INPUT_BYTES: usize = 12 + 255;
/// Driver-key name query output: `ConnectionIndex`, `ActualLength`, UTF-16 name.
const DRIVER_KEY_OUTPUT_BYTES: usize = 1028;
/// Guard against a malformed child chain.
const MAX_HUB_CHILDREN: usize = 64;

/// USB4 port state is owned by the `Usb4DeviceRouter` stack; the platform
/// exposes no user-mode enumeration for it, so `usb ports` reports the router
/// instead of guessing a port list.
const USB4_PORT_ENUMERATION_UNAVAILABLE: &str =
    "USB4 port state is owned by the Usb4DeviceRouter stack; the platform exposes no user-mode \
     port enumeration for it";

/// Read-only USB port inventory for this machine.
#[derive(Clone, Copy, Debug, Default)]
pub struct WindowsUsbPorts;

impl WindowsUsbPorts {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl UsbPortInventoryControl for WindowsUsbPorts {
    fn usb_port_inventory(&self) -> Result<UsbPortInventory> {
        read_inventory()
    }
}

/// Record the `usb.ports` capability row from a cheap, read-only probe.
///
/// The probe only enumerates USB hub device interfaces; it never opens a hub and
/// never walks a port tree, so capability discovery stays bounded even with a
/// dock attached.
pub fn record_usb_port_capabilities(capabilities: &mut CapabilitySet) -> Result<()> {
    // `record` yields `Option<Capability>`; the probe only needs the write.
    match hub_interfaces() {
        Ok(hubs) if !hubs.is_empty() => {
            capabilities.record(
                "usb.ports",
                Availability::Available,
                Some(format!(
                    "{} USB hub device interface(s) enumerate; ports are read per hub on demand",
                    hubs.len()
                )),
            );
        }
        Ok(_) => {
            capabilities.record(
                "usb.ports",
                Availability::Unavailable,
                Some("no USB hub device interface enumerates".into()),
            );
        }
        Err(error) => {
            capabilities.record(
                "usb.ports",
                Availability::Unavailable,
                Some(error.to_string()),
            );
        }
    }
    Ok(())
}

struct HubInterface {
    instance_id: String,
    path: Vec<u16>,
}

/// One hub's parent relation plus its probe outcome.
struct HubProbe {
    parent: Option<String>,
    record: Result<UsbHub>,
}

fn read_inventory() -> Result<UsbPortInventory> {
    let hubs = hub_interfaces()?;
    let mut probes = Vec::with_capacity(hubs.len());
    for hub in &hubs {
        probes.push(HubProbe {
            parent: devnode_property_string(&hub.instance_id, &DEVPKEY_Device_Parent),
            record: probe_hub(hub),
        });
    }

    let routers = usb4_host_routers();
    let controllers = group_controllers(&hubs, probes, &routers);
    let (pd_controller, per_port_unavailable) = power_delivery();

    Ok(UsbPortInventory {
        controllers,
        power_delivery: PowerDeliveryReport {
            pd_controller,
            usb4_host_routers: routers,
            per_port_unavailable: Some(per_port_unavailable),
        },
    })
}

/// Fold every hub under the controller it ultimately hangs from. A hub whose
/// parent is another enumerated hub is a child hub; anything else is the
/// controller devnode.
///
/// A hub that cannot be read (for example when the process may not open its
/// device interface) is reported on its controller as `ports_unavailable` with
/// the channel error, so one unreadable hub never hides the rest of the tree.
fn group_controllers(
    hubs: &[HubInterface],
    probes: Vec<HubProbe>,
    routers: &[String],
) -> Vec<UsbController> {
    let mut by_instance: std::collections::BTreeMap<&str, usize> =
        std::collections::BTreeMap::new();
    for (index, hub) in hubs.iter().enumerate() {
        by_instance.insert(hub.instance_id.as_str(), index);
    }

    let mut grouped: std::collections::BTreeMap<String, Vec<usize>> =
        std::collections::BTreeMap::new();
    for (index, hub) in hubs.iter().enumerate() {
        // Without a reported parent the hub itself becomes the group root, and
        // the entry carries that disclosure instead of inventing an id.
        let root_is_hub = probes[index].parent.is_none();
        let mut controller = probes[index]
            .parent
            .clone()
            .unwrap_or_else(|| hub.instance_id.clone());
        let mut depth = 0;
        while let Some(parent_index) = by_instance.get(controller.as_str()) {
            match probes[*parent_index].parent.clone() {
                Some(parent) if !parent.is_empty() => controller = parent,
                _ => break,
            }
            depth += 1;
            if depth > 8 {
                break;
            }
        }
        if root_is_hub {
            grouped
                .entry(hub.instance_id.clone())
                .or_default()
                .push(index);
        } else {
            grouped.entry(controller).or_default().push(index);
        }
    }

    let mut records: Vec<Option<HubProbe>> = probes.into_iter().map(Some).collect();
    let mut controllers = Vec::with_capacity(grouped.len() + routers.len());
    for (instance_id, members) in grouped {
        let driver = devnode_property_string(&instance_id, &DEVPKEY_Device_Service);
        let description = devnode_property_string(&instance_id, &DEVPKEY_Device_DeviceDesc)
            .unwrap_or_else(|| instance_id.clone());
        let kind = controller_kind(driver.as_deref());
        let mut child_hubs = Vec::with_capacity(members.len());
        let mut failures = Vec::new();
        for member in members {
            if let Some(HubProbe { record, .. }) = records[member].take() {
                match record {
                    Ok(hub) => child_hubs.push(hub),
                    Err(error) => failures.push(error.to_string()),
                }
            }
        }
        let ports_unavailable = if failures.is_empty() {
            None
        } else {
            Some(failures.join("; "))
        };
        controllers.push(UsbController {
            instance_id,
            description,
            driver,
            kind,
            hubs: child_hubs,
            ports_unavailable,
        });
    }

    // USB4 host routers own no hub device interface, so they come from the USB
    // class itself with an explicit "no user-mode port enumeration" note.
    for instance_id in routers {
        if controllers
            .iter()
            .any(|controller| &controller.instance_id == instance_id)
        {
            continue;
        }
        controllers.push(UsbController {
            description: devnode_property_string(instance_id, &DEVPKEY_Device_DeviceDesc)
                .unwrap_or_else(|| instance_id.clone()),
            driver: devnode_property_string(instance_id, &DEVPKEY_Device_Service),
            kind: UsbControllerKind::Usb4HostRouter,
            hubs: Vec::new(),
            ports_unavailable: Some(USB4_PORT_ENUMERATION_UNAVAILABLE.into()),
            instance_id: instance_id.clone(),
        });
    }
    controllers
}

fn controller_kind(driver: Option<&str>) -> UsbControllerKind {
    match driver {
        Some("USBXHCI") => UsbControllerKind::Xhci,
        Some(service) if service.starts_with("Usb4") => UsbControllerKind::Usb4HostRouter,
        _ => UsbControllerKind::Other,
    }
}

fn probe_hub(hub: &HubInterface) -> Result<UsbHub> {
    let handle = HubHandle::open(&hub.path)?;
    let mut output = vec![0u8; IOCTL_OUTPUT_BYTES];
    let length = handle.ioctl(IOCTL_USB_GET_NODE_INFORMATION, &[], 0, &mut output)?;
    let facts = decode_hub_node_information(&output[..length]).ok_or_else(|| {
        LctrlError::ChannelUnavailable {
            channel: format!(
                "USB hub {} returned an unrecognised node-information buffer",
                hub.instance_id
            ),
        }
    })?;

    // A nested hub's ports hang off that hub, not off the controller, so the
    // child devnodes of this hub are the only exact join target.
    let children = hub_children(&hub.instance_id);

    let mut ports = Vec::with_capacity(usize::from(facts.port_count));
    let mut input = vec![0u8; IOCTL_OUTPUT_BYTES];
    for number in 1..=facts.port_count {
        input.fill(0);
        input[..4].copy_from_slice(&u32::from(number).to_le_bytes());
        output.fill(0);
        let length = handle.ioctl(
            IOCTL_USB_GET_NODE_CONNECTION_INFORMATION_EX,
            &input,
            IOCTL_OUTPUT_BYTES as u32,
            &mut output,
        )?;
        let port = decode_port_connection_information(&output[..length]).ok_or_else(|| {
            LctrlError::ChannelUnavailable {
                channel: format!(
                    "USB hub {} port {number} returned an unrecognised connection buffer",
                    hub.instance_id
                ),
            }
        })?;

        let device = port.descriptor.map(|descriptor| {
            let driver_key = handle.port_driver_key(number);
            let child = driver_key.as_deref().and_then(|key| {
                children
                    .iter()
                    .find(|child| child.driver_key.as_deref() == Some(key))
            });
            UsbPortDevice {
                instance_id: child.map(|child| child.instance_id.clone()),
                description: child.and_then(|child| child.description.clone()),
                vendor_id: descriptor.vendor_id,
                product_id: descriptor.product_id,
                usb_revision: format_usb_revision(descriptor.bcd_usb),
                is_hub: descriptor.device_is_hub,
                max_power_ma: handle
                    .port_configuration(number)
                    .map(|configuration| max_power_ma(configuration.max_power_raw, descriptor.bcd_usb)),
            }
        });

        ports.push(UsbPort {
            number,
            connection: port.connection,
            speed: port.speed,
            device,
            power_delivery: None,
        });
    }

    Ok(UsbHub {
        instance_id: hub.instance_id.clone(),
        port_count: facts.port_count,
        bus_powered: facts.bus_powered,
        ports,
    })
}

fn hub_interfaces() -> Result<Vec<HubInterface>> {
    let set = device_set(
        &GUID_DEVINTERFACE_USB_HUB,
        DIGCF_PRESENT | DIGCF_DEVICEINTERFACE,
    )
    .ok_or_else(|| channel_error("USB hub device-interface enumeration"))?;
    let mut hubs = Vec::new();
    let mut index = 0u32;
    loop {
        let mut interface = zeroed::<SP_DEVICE_INTERFACE_DATA>();
        interface.cbSize = size_of::<SP_DEVICE_INTERFACE_DATA>() as u32;
        let ok = unsafe {
            SetupDiEnumDeviceInterfaces(
                set.0,
                std::ptr::null(),
                &GUID_DEVINTERFACE_USB_HUB,
                index,
                &mut interface,
            )
        };
        if ok == 0 {
            break;
        }
        index += 1;

        let mut detail = vec![0u8; 2048];
        let capacity = size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;
        detail[..4].copy_from_slice(&capacity.to_le_bytes());
        let mut info = zeroed::<SP_DEVINFO_DATA>();
        info.cbSize = size_of::<SP_DEVINFO_DATA>() as u32;
        let mut required = 0u32;
        let ok = unsafe {
            SetupDiGetDeviceInterfaceDetailW(
                set.0,
                &interface,
                detail
                    .as_mut_ptr()
                    .cast::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>(),
                detail.len() as u32,
                &mut required,
                &mut info,
            )
        };
        if ok == 0 {
            return Err(channel_error("USB hub interface path"));
        }

        // The instance id comes from the devnode the detail call returned; the
        // interface path is only an open handle, not a name to parse.
        let instance_id = match setupapi_property(set.0, &info, &DEVPKEY_Device_InstanceId)
            .and_then(|property| property.as_string())
        {
            Some(instance_id) if !instance_id.is_empty() => instance_id,
            _ => return Err(channel_error("USB hub device instance id")),
        };
        let mut path: Vec<u16> = detail[4..]
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .take_while(|unit| *unit != 0)
            .collect();
        path.push(0);
        hubs.push(HubInterface { instance_id, path });
    }
    hubs.sort_by(|left, right| left.instance_id.cmp(&right.instance_id));
    Ok(hubs)
}

fn usb4_host_routers() -> Vec<String> {
    let Some(set) = device_set(&GUID_DEVCLASS_USB, DIGCF_PRESENT) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut index = 0u32;
    loop {
        let mut info = zeroed::<SP_DEVINFO_DATA>();
        info.cbSize = size_of::<SP_DEVINFO_DATA>() as u32;
        if unsafe { SetupDiEnumDeviceInfo(set.0, index, &mut info) } == 0 {
            break;
        }
        index += 1;
        let service = setupapi_property(set.0, &info, &DEVPKEY_Device_Service)
            .and_then(|property| property.as_string());
        if service.as_deref() != Some("Usb4HostRouter") {
            continue;
        }
        if let Some(instance_id) = setupapi_property(set.0, &info, &DEVPKEY_Device_InstanceId)
            .and_then(|property| property.as_string())
        {
            out.push(instance_id);
        }
    }
    out.sort();
    out
}

/// The platform's UCSI power-delivery controller plus the measured reason no
/// port reports a power contract.
fn power_delivery() -> (Option<UsbPdController>, String) {
    let controller = ucm_controller();
    let connector_interfaces = ucm_connector_interfaces();
    let reason = match (&controller, connector_interfaces) {
        (Some(controller), 0) => format!(
            "UCSI power-delivery device {} is present, but it registers no user-mode connector \
             interface (0 device interface(s) under the UCM class GUID) and USB hub port data \
             carries no charger type; no port reports PD or BC1.2",
            controller.instance_id
        ),
        (Some(controller), count) => format!(
            "UCSI power-delivery device {} registers {count} connector interface(s), but this \
             command has no decoded UCSI connector-status contract and USB hub port data carries \
             no charger type; no port reports PD or BC1.2",
            controller.instance_id
        ),
        (None, _) => "no UCSI power-delivery device is present and USB hub port data carries no \
                      charger type; no port reports PD or BC1.2"
            .into(),
    };
    (controller, reason)
}

fn ucm_controller() -> Option<UsbPdController> {
    let set = device_set(&GUID_DEVCLASS_UCM, DIGCF_PRESENT)?;
    // The UCM class carries the platform's UCSI PPM; the first enumerated member
    // is that device.
    let mut info = zeroed::<SP_DEVINFO_DATA>();
    info.cbSize = size_of::<SP_DEVINFO_DATA>() as u32;
    if unsafe { SetupDiEnumDeviceInfo(set.0, 0, &mut info) } == 0 {
        return None;
    }
    let instance_id = setupapi_property(set.0, &info, &DEVPKEY_Device_InstanceId)
        .and_then(|property| property.as_string())?;
    Some(UsbPdController {
        driver: setupapi_property(set.0, &info, &DEVPKEY_Device_Service)
            .and_then(|property| property.as_string()),
        firmware_version: setupapi_property(set.0, &info, &DEVPKEY_Device_FirmwareVersion)
            .and_then(|property| property.as_string()),
        instance_id,
    })
}

fn ucm_connector_interfaces() -> usize {
    let Some(set) = device_set(&GUID_DEVCLASS_UCM, DIGCF_PRESENT | DIGCF_DEVICEINTERFACE) else {
        return 0;
    };
    let mut count = 0usize;
    let mut index = 0u32;
    loop {
        let mut interface = zeroed::<SP_DEVICE_INTERFACE_DATA>();
        interface.cbSize = size_of::<SP_DEVICE_INTERFACE_DATA>() as u32;
        if unsafe {
            SetupDiEnumDeviceInterfaces(
                set.0,
                std::ptr::null(),
                &GUID_DEVCLASS_UCM,
                index,
                &mut interface,
            )
        } == 0
        {
            return count;
        }
        index += 1;
        count += 1;
    }
}

struct HubChild {
    driver_key: Option<String>,
    instance_id: String,
    description: Option<String>,
}

fn hub_children(instance_id: &str) -> Vec<HubChild> {
    let Some(devinst) = locate(instance_id) else {
        return Vec::new();
    };
    let mut children = Vec::new();
    let mut child = 0u32;
    if unsafe { CM_Get_Child(&mut child, devinst, 0) } != CR_SUCCESS {
        return children;
    }
    for _ in 0..MAX_HUB_CHILDREN {
        if let Some(child_id) = device_id(child) {
            children.push(HubChild {
                driver_key: devnode_property(child, &DEVPKEY_Device_Driver)
                    .and_then(|property| property.as_string()),
                description: devnode_property(child, &DEVPKEY_Device_BusReportedDeviceDesc)
                    .and_then(|property| property.as_string()),
                instance_id: child_id,
            });
        }
        let mut sibling = 0u32;
        if unsafe { CM_Get_Sibling(&mut sibling, child, 0) } != CR_SUCCESS {
            break;
        }
        child = sibling;
    }
    children
}

struct HubHandle(HANDLE);

impl HubHandle {
    fn open(path: &[u16]) -> Result<Self> {
        // Desired access 0: every hub IOCTL used here is FILE_ANY_ACCESS, and the
        // command must not be able to change a port.
        let handle = unsafe {
            CreateFileW(
                path.as_ptr(),
                0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                std::ptr::null(),
                OPEN_EXISTING,
                0,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(channel_error("USB hub device interface open"));
        }
        Ok(Self(handle))
    }

    fn ioctl(&self, code: u32, input: &[u8], input_length: u32, output: &mut [u8]) -> Result<usize> {
        let mut returned = 0u32;
        let ok = unsafe {
            DeviceIoControl(
                self.0,
                code,
                input.as_ptr().cast::<c_void>(),
                input_length,
                output.as_mut_ptr().cast::<c_void>(),
                output.len() as u32,
                &mut returned,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(channel_error(&format!("USB hub IOCTL {code:#010x}")));
        }
        Ok(returned as usize)
    }

    fn port_driver_key(&self, port: u8) -> Option<String> {
        let mut input = [0u8; 4];
        input.copy_from_slice(&u32::from(port).to_le_bytes());
        let mut output = vec![0u8; DRIVER_KEY_OUTPUT_BYTES];
        let length = self
            .ioctl(
                IOCTL_USB_GET_NODE_CONNECTION_DRIVERKEY_NAME,
                &input,
                4,
                &mut output,
            )
            .ok()?;
        if length < 8 {
            return None;
        }
        let actual = u32::from_le_bytes([output[4], output[5], output[6], output[7]]) as usize;
        if actual < 2 {
            return None;
        }
        let end = (8 + actual - 2).min(length);
        let name: Vec<u16> = output[8..end]
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .take_while(|unit| *unit != 0)
            .collect();
        (!name.is_empty()).then(|| String::from_utf16_lossy(&name))
    }

    fn port_configuration(&self, port: u8) -> Option<DeviceConfigurationFacts> {
        let mut input = vec![0u8; 1024];
        input[..4].copy_from_slice(&u32::from(port).to_le_bytes());
        // GET_DESCRIPTOR(USB_CONFIGURATION, index 0), 255 bytes.
        input[4] = 0x80;
        input[5] = 0x06;
        input[6..8].copy_from_slice(&0x0200u16.to_le_bytes());
        input[8..10].copy_from_slice(&0u16.to_le_bytes());
        input[10..12].copy_from_slice(&255u16.to_le_bytes());
        let mut output = vec![0u8; 1024];
        let length = self
            .ioctl(
                IOCTL_USB_GET_DESCRIPTOR_FROM_NODE_CONNECTION,
                &input,
                DESCRIPTOR_REQUEST_INPUT_BYTES as u32,
                &mut output,
            )
            .ok()?;
        decode_config_descriptor(&output[..length])
    }
}

impl Drop for HubHandle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

struct DeviceSet(HDEVINFO);

impl Drop for DeviceSet {
    fn drop(&mut self) {
        unsafe {
            SetupDiDestroyDeviceInfoList(self.0);
        }
    }
}

fn device_set(
    guid: &windows_sys::core::GUID,
    flags: SETUP_DI_GET_CLASS_DEVS_FLAGS,
) -> Option<DeviceSet> {
    let set = unsafe { SetupDiGetClassDevsW(guid, std::ptr::null(), std::ptr::null_mut(), flags) };
    // `HDEVINFO` is `isize` in windows-sys; `INVALID_HANDLE_VALUE` is a
    // `*mut c_void`, so compare against the integer form.
    (set != -1isize).then_some(DeviceSet(set))
}

struct RawProperty {
    kind: u32,
    bytes: Vec<u8>,
}

impl RawProperty {
    fn as_string(&self) -> Option<String> {
        if self.kind != DEVPROP_TYPE_STRING && self.kind != DEVPROP_TYPE_STRING_INDIRECT {
            return None;
        }
        let units: Vec<u16> = self
            .bytes
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .take_while(|unit| *unit != 0)
            .collect();
        (!units.is_empty()).then(|| String::from_utf16_lossy(&units))
    }
}

fn setupapi_property(
    set: HDEVINFO,
    info: &SP_DEVINFO_DATA,
    key: &DEVPROPKEY,
) -> Option<RawProperty> {
    for capacity in [256usize, 1024, 8192] {
        let mut kind = 0u32;
        let mut buffer = vec![0u8; capacity];
        let mut required = 0u32;
        let ok = unsafe {
            SetupDiGetDevicePropertyW(
                set,
                info,
                key,
                &mut kind,
                buffer.as_mut_ptr(),
                capacity as u32,
                &mut required,
                0,
            )
        };
        if ok != 0 {
            return Some(RawProperty { kind, bytes: buffer });
        }
        if required as usize > capacity {
            continue;
        }
        return None;
    }
    None
}

fn devnode_property(devinst: u32, key: &DEVPROPKEY) -> Option<RawProperty> {
    for capacity in [256usize, 1024, 8192] {
        let mut kind = 0u32;
        let mut buffer = vec![0u8; capacity];
        let mut size = capacity as u32;
        let status = unsafe {
            CM_Get_DevNode_PropertyW(devinst, key, &mut kind, buffer.as_mut_ptr(), &mut size, 0)
        };
        if status == CR_SUCCESS {
            return Some(RawProperty { kind, bytes: buffer });
        }
        if size as usize > capacity {
            continue;
        }
        return None;
    }
    None
}

fn devnode_property_string(instance_id: &str, key: &DEVPROPKEY) -> Option<String> {
    let devinst = locate(instance_id)?;
    devnode_property(devinst, key).and_then(|property| property.as_string())
}

fn locate(instance_id: &str) -> Option<u32> {
    let name = wide(instance_id);
    let mut devinst = 0u32;
    let status = unsafe { CM_Locate_DevNodeW(&mut devinst, name.as_ptr(), 0) };
    (status == CR_SUCCESS).then_some(devinst)
}

fn device_id(devinst: u32) -> Option<String> {
    let mut buffer = vec![0u16; MAX_DEVICE_ID_LEN as usize];
    let status = unsafe { CM_Get_Device_IDW(devinst, buffer.as_mut_ptr(), MAX_DEVICE_ID_LEN, 0) };
    if status != CR_SUCCESS {
        return None;
    }
    let length = buffer.iter().position(|unit| *unit == 0)?;
    (length > 0).then(|| String::from_utf16_lossy(&buffer[..length]))
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn zeroed<T>() -> T {
    // SAFETY: every caller uses a POD Win32 structure whose all-zero value is
    // valid, and sets `cbSize` before the structure is passed to the API.
    unsafe { std::mem::zeroed() }
}

fn channel_error(channel: &str) -> LctrlError {
    LctrlError::ChannelUnavailable {
        channel: format!("{channel}: {}", io::Error::last_os_error()),
    }
}
