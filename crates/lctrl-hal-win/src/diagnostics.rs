use lctrl_core::{DiagnosticKind, DiagnosticOutcome, DiagnosticResult, Result, UpdateCapability};
use lctrl_hal::{DiagnosticsControl, UpdateControl};

use crate::WmiTransport;

/// Read-only Windows inventory diagnostics built on public CIM/WMI classes.
pub struct WindowsSystemInventory<W> {
    transport: W,
}

impl<W> WindowsSystemInventory<W> {
    pub const fn new(transport: W) -> Self {
        Self { transport }
    }
}

impl<W> DiagnosticsControl for WindowsSystemInventory<W>
where
    W: WmiTransport,
{
    fn diagnostic_items(&self) -> Result<Vec<DiagnosticKind>> {
        Ok(vec![
            DiagnosticKind::Battery,
            DiagnosticKind::Thermal,
            DiagnosticKind::Storage,
            DiagnosticKind::Memory,
            DiagnosticKind::Firmware,
            DiagnosticKind::Network,
        ])
    }

    fn run_diagnostics(&self, items: &[DiagnosticKind]) -> Result<Vec<DiagnosticResult>> {
        Ok(items
            .iter()
            .copied()
            .map(|kind| {
                let (namespace, query, label) = diagnostic_query(kind);
                match self.transport.query(namespace, query) {
                    Ok(objects) if objects.is_empty() => DiagnosticResult {
                        kind,
                        outcome: DiagnosticOutcome::Unavailable,
                        detail: format!("no {label} records were returned"),
                    },
                    Ok(objects) => DiagnosticResult {
                        kind,
                        outcome: DiagnosticOutcome::Warning,
                        detail: format!(
                            "{} {label} record(s) inventoried; deep proprietary-driver diagnostics are excluded",
                            objects.len()
                        ),
                    },
                    Err(error) => DiagnosticResult {
                        kind,
                        outcome: DiagnosticOutcome::Unavailable,
                        detail: error.to_string(),
                    },
                }
            })
            .collect())
    }
}

impl<W> UpdateControl for WindowsSystemInventory<W>
where
    W: WmiTransport,
{
    fn update_capability(&self) -> Result<UpdateCapability> {
        Ok(UpdateCapability::Unavailable {
            reason: "no authenticated public Lenovo update catalog/manifest contract is specified; private MCP packages and firmware flashing are excluded".into(),
        })
    }
}

fn diagnostic_query(kind: DiagnosticKind) -> (&'static str, &'static str, &'static str) {
    match kind {
        DiagnosticKind::Battery => (
            r"ROOT\CIMV2",
            "SELECT BatteryStatus, EstimatedChargeRemaining FROM Win32_Battery",
            "battery",
        ),
        DiagnosticKind::Thermal => (
            r"ROOT\WMI",
            "SELECT CurrentTemperature, CriticalTripPoint FROM MSAcpi_ThermalZoneTemperature",
            "thermal-zone",
        ),
        DiagnosticKind::Storage => (
            r"ROOT\CIMV2",
            "SELECT Model, Status, Size FROM Win32_DiskDrive",
            "storage-device",
        ),
        DiagnosticKind::Memory => (
            r"ROOT\CIMV2",
            "SELECT Capacity, Status FROM Win32_PhysicalMemory",
            "memory-device",
        ),
        DiagnosticKind::Firmware => (
            r"ROOT\CIMV2",
            "SELECT SMBIOSBIOSVersion, Status FROM Win32_BIOS",
            "firmware",
        ),
        DiagnosticKind::Network => (
            r"ROOT\CIMV2",
            "SELECT Name, NetEnabled FROM Win32_NetworkAdapter WHERE PhysicalAdapter = TRUE",
            "network-adapter",
        ),
    }
}
