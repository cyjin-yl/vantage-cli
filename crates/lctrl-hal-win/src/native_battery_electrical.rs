//! Read-only Microsoft battery-class electrical observations.
//!
//! References: Microsoft Learn's Battery Information, BATTERY_INFORMATION,
//! BATTERY_STATUS, and IOCTL_BATTERY_QUERY_{TAG,INFORMATION,STATUS} contracts.
//! No vendor controls, identifiers, nominal-voltage substitutions, or inferred
//! current are used. The raw power-state bitfield is not interpreted as a rate.
//!
//! A single process-wide worker owns all native resources. Callers wait at most
//! two seconds for a reply; a busy worker is rejected rather than queued. This
//! bounds caller waiting, NOT Windows discovery/open/I/O execution. Windows can
//! leave that worker blocked until the provider returns or the process exits.
//! Timeout abandons the response, not the operation; no cancellation or worker
//! termination is claimed, and repeated calls cannot spawn additional workers.

#[cfg(any(windows, test))]
use lctrl_core::BatterySourceIssue;
use lctrl_core::{BatteryElectricalObservation, LctrlError, Result};
use lctrl_hal::BatteryElectricalSource;

#[derive(Clone, Copy, Debug, Default)]
pub struct NativeBatteryElectrical;

#[cfg(any(windows, test))]
const SYSTEM_BATTERY: u32 = 0x8000_0000;
#[cfg(any(windows, test))]
const CAPACITY_RELATIVE: u32 = 0x4000_0000;
#[cfg(any(windows, test))]
const SHORT_TERM: u32 = 0x2000_0000;

fn unavailable(detail: &str) -> LctrlError {
    LctrlError::ChannelUnavailable {
        channel: format!("Windows battery class: {detail}"),
    }
}

#[cfg(any(windows, test))]
fn eligible(capabilities: u32) -> bool {
    capabilities & SYSTEM_BATTERY != 0 && capabilities & SHORT_TERM == 0
}

#[cfg(any(windows, test))]
fn battery_tag(reply: Result<[u8; 4]>) -> Result<Option<u32>> {
    match reply {
        Ok(bytes) => {
            let tag = u32::from_le_bytes(bytes);
            Ok((tag != 0).then_some(tag))
        }
        // QUERY_TAG documents ERROR_FILE_NOT_FOUND for an empty port.
        Err(LctrlError::Io(error)) if error.raw_os_error() == Some(2) => Ok(None),
        Err(error) => Err(error),
    }
}

#[cfg(any(windows, test))]
fn admit_interface(index: u32) -> Result<()> {
    if index >= 100 {
        return Err(unavailable(
            "battery interface enumeration exceeds 100 devices",
        ));
    }
    Ok(())
}

#[cfg(any(windows, test))]
fn admit_system_battery(already_selected: bool) -> Result<()> {
    if already_selected {
        return Err(unavailable(
            "multiple non-short-term system batteries; electrical selection is ambiguous",
        ));
    }
    Ok(())
}

#[cfg(any(windows, test))]
fn attach_temperature(observation: &mut BatteryElectricalObservation, reply: Result<[u8; 4]>) {
    match reply {
        // Microsoft documents ULONG tenths kelvin, not a temperature sentinel.
        Ok(bytes) => observation.temperature_deci_kelvin = Some(u32::from_le_bytes(bytes)),
        Err(LctrlError::Io(error)) if error.raw_os_error() == Some(1) => {}
        Err(error) => observation.source_issues.push(BatterySourceIssue {
            source: "Windows battery class temperature",
            detail: error.to_string(),
        }),
    }
}

#[cfg(any(windows, test))]
fn project(capabilities: u32, status: &[u8]) -> Result<BatteryElectricalObservation> {
    if !eligible(capabilities) {
        return Err(unavailable("not a non-short-term system battery"));
    }
    if status.len() != 16 {
        return Err(unavailable("malformed BATTERY_STATUS reply length"));
    }
    let word = |offset| u32::from_le_bytes(status[offset..offset + 4].try_into().unwrap());
    let voltage = word(8);
    let rate = word(12) as i32;
    Ok(BatteryElectricalObservation {
        voltage_mv: (voltage != u32::MAX).then_some(voltage),
        rate_mw: (capabilities & CAPACITY_RELATIVE == 0 && rate != i32::MIN).then_some(rate),
        power_state: word(0),
        temperature_deci_kelvin: None,
        source_issues: Vec::new(),
    })
}

impl BatteryElectricalSource for NativeBatteryElectrical {
    fn read_electrical(&self) -> Result<BatteryElectricalObservation> {
        #[cfg(windows)]
        {
            native::read()
        }
        #[cfg(not(windows))]
        {
            Err(unavailable("requires Windows"))
        }
    }
}

#[cfg(windows)]
mod native {
    use super::*;
    use std::{
        ffi::c_void,
        ptr,
        sync::{
            Arc, LazyLock,
            atomic::{AtomicBool, Ordering},
            mpsc,
        },
        time::Duration,
    };

    type Handle = *mut c_void;
    #[repr(C)]
    struct Guid {
        a: u32,
        b: u16,
        c: u16,
        d: [u8; 8],
    }
    // GUID_DEVCLASS_BATTERY, documented battery-class interface enumeration GUID.
    const BATTERY: Guid = Guid {
        a: 0x72631e54,
        b: 0x78a4,
        c: 0x11d0,
        d: [0xbc, 0xf7, 0, 0xaa, 0, 0xb7, 0xb3, 0x2a],
    };
    #[repr(C)]
    struct Interface {
        size: u32,
        class: Guid,
        flags: u32,
        reserved: usize,
    }
    #[link(name = "setupapi")]
    unsafe extern "system" {
        fn SetupDiGetClassDevsW(
            class: *const Guid,
            enumerator: *const u16,
            parent: Handle,
            flags: u32,
        ) -> Handle;
        fn SetupDiEnumDeviceInterfaces(
            set: Handle,
            device: *const c_void,
            class: *const Guid,
            index: u32,
            data: *mut Interface,
        ) -> i32;
        fn SetupDiGetDeviceInterfaceDetailW(
            set: Handle,
            interface: *const Interface,
            detail: *mut c_void,
            size: u32,
            required: *mut u32,
            device: *mut c_void,
        ) -> i32;
        fn SetupDiDestroyDeviceInfoList(set: Handle) -> i32;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateFileW(
            path: *const u16,
            access: u32,
            share: u32,
            security: *const c_void,
            disposition: u32,
            flags: u32,
            template: Handle,
        ) -> Handle;
        fn DeviceIoControl(
            handle: Handle,
            code: u32,
            input: *const c_void,
            input_size: u32,
            output: *mut c_void,
            output_size: u32,
            returned: *mut u32,
            overlapped: *mut c_void,
        ) -> i32;
        fn CloseHandle(handle: Handle) -> i32;
    }
    struct Device(Handle);
    impl Drop for Device {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
    struct Devices(Handle);
    impl Drop for Devices {
        fn drop(&mut self) {
            unsafe {
                SetupDiDestroyDeviceInfoList(self.0);
            }
        }
    }

    fn query<const N: usize>(device: &Device, code: u32, input: &[u32]) -> Result<[u8; N]> {
        let mut output = [0u8; N];
        let mut returned = 0;
        let ok = unsafe {
            DeviceIoControl(
                device.0,
                code,
                input.as_ptr().cast(),
                std::mem::size_of_val(input) as u32,
                output.as_mut_ptr().cast(),
                N as u32,
                &mut returned,
                ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        if returned as usize != N {
            return Err(unavailable("malformed or short battery IOCTL reply"));
        }
        Ok(output)
    }

    fn collect() -> Result<BatteryElectricalObservation> {
        let raw = unsafe { SetupDiGetClassDevsW(&BATTERY, ptr::null(), ptr::null_mut(), 0x12) }; // PRESENT | DEVICEINTERFACE
        if raw == -1isize as Handle {
            return Err(std::io::Error::last_os_error().into());
        }
        let set = Devices(raw);
        let mut selected: Option<(Device, u32, u32)> = None;
        // Inspect at most 100 interfaces, plus one existence check so a
        // truncated enumeration can never select an apparently unique battery.
        for index in 0..=100 {
            let mut interface = Interface {
                size: std::mem::size_of::<Interface>() as u32,
                class: Guid {
                    a: 0,
                    b: 0,
                    c: 0,
                    d: [0; 8],
                },
                flags: 0,
                reserved: 0,
            };
            if unsafe {
                SetupDiEnumDeviceInterfaces(set.0, ptr::null(), &BATTERY, index, &mut interface)
            } == 0
            {
                let error = std::io::Error::last_os_error();
                if error.raw_os_error() == Some(259) {
                    break;
                } // ERROR_NO_MORE_ITEMS
                return Err(error.into());
            }
            admit_interface(index)?;
            let mut required = 0;
            let ok = unsafe {
                SetupDiGetDeviceInterfaceDetailW(
                    set.0,
                    &interface,
                    ptr::null_mut(),
                    0,
                    &mut required,
                    ptr::null_mut(),
                )
            };
            if ok != 0 {
                return Err(unavailable("unexpected interface sizing success"));
            }
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() != Some(122) {
                return Err(error.into());
            }
            if !(8..=1024 * 1024).contains(&required) {
                return Err(unavailable("malformed interface detail length"));
            }
            // Pointer-sized words provide native structure alignment; DevicePath
            // starts at byte 4 on both x86 and x64, independently of cbSize.
            let mut detail = vec![
                0usize;
                (required as usize + std::mem::size_of::<usize>() - 1)
                    / std::mem::size_of::<usize>()
            ];
            let bytes = unsafe {
                std::slice::from_raw_parts_mut(detail.as_mut_ptr().cast::<u8>(), required as usize)
            };
            let cb_size = if cfg!(target_pointer_width = "64") {
                8u32
            } else {
                6u32
            };
            bytes[..4].copy_from_slice(&cb_size.to_le_bytes());
            let capacity = required;
            if unsafe {
                SetupDiGetDeviceInterfaceDetailW(
                    set.0,
                    &interface,
                    bytes.as_mut_ptr().cast(),
                    capacity,
                    &mut required,
                    ptr::null_mut(),
                )
            } == 0
            {
                return Err(std::io::Error::last_os_error().into());
            }
            if required > capacity || required < 8 || required % 2 != 0 {
                return Err(unavailable("malformed interface detail reply"));
            }
            let path = &bytes[4..required as usize];
            if !path.chunks_exact(2).any(|pair| pair == [0, 0]) {
                return Err(unavailable("unterminated device interface path"));
            }
            let raw = unsafe {
                CreateFileW(
                    bytes.as_ptr().add(4).cast(),
                    0x8000_0000,
                    3,
                    ptr::null(),
                    3,
                    0,
                    ptr::null_mut(),
                )
            }; // GENERIC_READ, SHARE_READ|WRITE, OPEN_EXISTING
            if raw == -1isize as Handle {
                return Err(std::io::Error::last_os_error().into());
            }
            let device = Device(raw);
            let Some(tag) = battery_tag(query::<4>(&device, 0x0029_4040, &[0]))? else {
                continue;
            };
            let info = query::<36>(&device, 0x0029_4044, &[tag, 0, 0])?; // BatteryInformation
            let capabilities = u32::from_le_bytes(info[..4].try_into().unwrap());
            if !eligible(capabilities) {
                continue;
            }
            admit_system_battery(selected.is_some())?;
            selected = Some((device, tag, capabilities));
        }
        let (device, tag, capabilities) =
            selected.ok_or_else(|| unavailable("no non-short-term system battery"))?;
        // BATTERY_WAIT_STATUS: zero timeout, no notification wait requested.
        let status = query::<16>(&device, 0x0029_404c, &[tag, 0, 0, 0, 0])?;
        let mut observation = project(capabilities, &status)?;
        // Same admitted handle and tag: no second-source identity association.
        attach_temperature(
            &mut observation,
            query::<4>(&device, 0x0029_4044, &[tag, 2, 0]),
        );
        Ok(observation)
    }

    struct Job {
        response: mpsc::SyncSender<Result<BatteryElectricalObservation>>,
        abandoned: Arc<AtomicBool>,
    }
    struct Worker {
        sender: mpsc::SyncSender<Job>,
        busy: Arc<AtomicBool>,
    }
    static WORKER: LazyLock<std::result::Result<Worker, std::io::Error>> = LazyLock::new(|| {
        let (sender, receiver) = mpsc::sync_channel::<Job>(1);
        let busy = Arc::new(AtomicBool::new(false));
        let worker_busy = Arc::clone(&busy);
        std::thread::Builder::new()
            .name("battery-class-reader".into())
            .spawn(move || {
                while let Ok(job) = receiver.recv() {
                    if !job.abandoned.load(Ordering::Acquire) {
                        let result = std::panic::catch_unwind(collect).unwrap_or_else(|_| {
                            Err(unavailable(
                                "reader operation panicked; no observation available",
                            ))
                        });
                        if !job.abandoned.load(Ordering::Acquire) {
                            let _ = job.response.send(result);
                        }
                    }
                    worker_busy.store(false, Ordering::Release);
                }
            })?;
        Ok(Worker { sender, busy })
    });

    pub(super) fn read() -> Result<BatteryElectricalObservation> {
        let worker = WORKER.as_ref().map_err(|error| {
            LctrlError::Io(match error.raw_os_error() {
                Some(code) => std::io::Error::from_raw_os_error(code),
                None => {
                    std::io::Error::new(error.kind(), format!("battery worker startup: {error}"))
                }
            })
        })?;
        if worker
            .busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(unavailable(
                "reader busy; prior Windows operation may still be running",
            ));
        }
        let (response, receiver) = mpsc::sync_channel(1);
        let abandoned = Arc::new(AtomicBool::new(false));
        if worker
            .sender
            .try_send(Job {
                response,
                abandoned: Arc::clone(&abandoned),
            })
            .is_err()
        {
            worker.busy.store(false, Ordering::Release);
            return Err(unavailable("reader worker unavailable"));
        }
        match receiver.recv_timeout(Duration::from_secs(2)) {
            Ok(result) => result,
            Err(error) => {
                abandoned.store(true, Ordering::Release);
                match error {
                    mpsc::RecvTimeoutError::Timeout => Err(unavailable(
                        "response wait exceeded two seconds; Windows operation is not cancelled and may remain running",
                    )),
                    mpsc::RecvTimeoutError::Disconnected => {
                        Err(unavailable("reader worker disconnected"))
                    }
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "native_battery_electrical_tests.rs"]
mod tests;
