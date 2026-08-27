//! A read-only GPUI control-center shell for lctrl.
//!
//! The GUI deliberately has no hardware mutation path. It renders the state
//! reported by [`lctrl_hal::Hal`] and marks every capability according to the
//! core availability value, including its explanation.

use gpui::{
    App, Application, Bounds, Context, Render, Window, WindowBounds, WindowOptions, div,
    prelude::*, px, rgb, size,
};
use lctrl_core::{Availability, CapabilitySet, HardwareInfo, Platform, Result};
use lctrl_hal::Hal;

// Palette tokens: a cool instrument-panel base, with one signal color and one
// caution color. Keeping these in one place makes the dashboard's visual
// language intentional and easy to retune without scattering literals.
const INK: u32 = 0x0b1118;
const SURFACE: u32 = 0x111b24;
const SURFACE_RAISED: u32 = 0x172531;
const RULE: u32 = 0x2a3b48;
const TEXT: u32 = 0xd8e4e8;
const MUTED: u32 = 0x7f98a2;
const SIGNAL: u32 = 0x54d6c7;
const CAUTION: u32 = 0xf2b66d;
const UNAVAILABLE: u32 = 0x53636c;
const WINDOW_WIDTH: f32 = 1240.0;
const WINDOW_HEIGHT: f32 = 820.0;
const SIDEBAR_WIDTH: f32 = 238.0;
const MARKER_SIZE: f32 = 8.0;
const FEATURE_COLUMN_WIDTH: f32 = 190.0;
const AVAILABILITY_COLUMN_WIDTH: f32 = 112.0;

/// Immutable data consumed by the dashboard.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DashboardSnapshot {
    /// The platform reported by the hardware abstraction layer.
    pub platform: Platform,
    /// Product identity reported by the hardware abstraction layer.
    pub hardware: HardwareInfo,
    /// Capabilities and their availability explanations.
    pub capabilities: CapabilitySet,
    /// Human-readable status for the safety banner.
    pub status_message: String,
}

impl DashboardSnapshot {
    /// Query the real platform, hardware identity, and capability set.
    ///
    /// Errors are returned unchanged from `lctrl-core`; this keeps callers from
    /// accidentally displaying a successful-looking snapshot for a failed
    /// query.
    pub fn from_hal(hal: &dyn Hal) -> Result<Self> {
        let platform = hal.platform();
        let hardware = hal.hardware_info()?;
        let capabilities = hal.capabilities()?;
        Ok(Self {
            platform,
            hardware,
            capabilities,
            status_message: "Live hardware status synchronized".to_string(),
        })
    }

    /// Build the conservative state used by the standalone executable until a
    /// platform HAL is supplied by the controller.
    #[must_use]
    pub fn unavailable(platform: Platform, status_message: impl Into<String>) -> Self {
        Self {
            platform,
            hardware: HardwareInfo::default(),
            capabilities: CapabilitySet::new(platform),
            status_message: status_message.into(),
        }
    }
}

/// Open the lctrl dashboard with a read-only snapshot.
pub fn run(snapshot: DashboardSnapshot) {
    Application::new().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            move |_, cx| cx.new(|_| Dashboard { snapshot }),
        )
        .expect("lctrl dashboard window should open");
        cx.activate(true);
    });
}

/// GPUI view for the technical control-center dashboard.
pub struct Dashboard {
    snapshot: DashboardSnapshot,
}

impl Render for Dashboard {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let snapshot = &self.snapshot;
        div()
            .size_full()
            .flex()
            .flex_row()
            .bg(rgb(INK))
            .text_color(rgb(TEXT))
            .font_family("Iosevka, IBM Plex Mono, monospace")
            .child(sidebar())
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .overflow_hidden()
                    .p_6()
                    .gap_5()
                    .child(identity_header(snapshot))
                    .child(safety_banner(&snapshot.status_message))
                    .child(capability_matrix(&snapshot.capabilities))
                    .child(telemetry_panel(snapshot)),
            )
    }
}

fn sidebar() -> impl IntoElement {
    let sections = [
        ("01", "OVERVIEW", true),
        ("02", "POWER", false),
        ("03", "PERFORMANCE", false),
        ("04", "DEVICES", false),
        ("05", "BIOS", false),
        ("06", "TUNING", false),
        ("07", "DIAGNOSTICS", false),
    ];

    div()
        .w(px(238.0))
        .flex_none()
        .flex()
        .flex_col()
        .bg(rgb(SURFACE))
        .border_r_1()
        .w(px(SIDEBAR_WIDTH))
        .p_5()
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .pb_6()
                .border_b_1()
                .border_color(rgb(RULE))
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(SIGNAL))
                        .child("LCTRL / CONTROL"),
                )
                .child(
                    div()
                        .text_xl()
                        .font_weight(gpui::FontWeight::BOLD)
                        .child("CENTER"),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(MUTED))
                        .child("READ-ONLY CONSOLE 0.1"),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .pt_5()
                .children(sections.into_iter().map(|(number, label, active)| {
                    let (background, color) = if active {
                        (rgb(SURFACE_RAISED), rgb(SIGNAL))
                    } else {
                        (rgb(SURFACE), rgb(MUTED))
                    };
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_3()
                        .px_3()
                        .py_2()
                        .bg(background)
                        .border_l_2()
                        .border_color(if active { rgb(SIGNAL) } else { rgb(SURFACE) })
                        .text_color(color)
                        .child(div().text_xs().child(number))
                        .child(div().text_sm().child(label))
                }))
                .child(
                    div()
                        .mt_6()
                        .pt_5()
                        .border_t_1()
                        .border_color(rgb(RULE))
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(div().text_xs().text_color(rgb(MUTED)).child("SESSION"))
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(CAUTION))
                                .child("●  OBSERVE ONLY"),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(UNAVAILABLE))
                                .child("Controls unlock only after a\nsafe dispatcher is wired."),
                        ),
                ),
        )
}

fn identity_header(snapshot: &DashboardSnapshot) -> impl IntoElement {
    let product = snapshot
        .hardware
        .product_name
        .as_deref()
        .unwrap_or("Hardware identity pending");
    let family = snapshot
        .hardware
        .family
        .as_deref()
        .unwrap_or("Unknown family");
    let bios = snapshot
        .hardware
        .bios_version
        .as_deref()
        .unwrap_or("BIOS not reported");
    let platform = platform_name(snapshot.platform);

    div()
        .flex()
        .flex_row()
        .justify_between()
        .items_end()
        .pb_4()
        .border_b_1()
        .border_color(rgb(RULE))
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(SIGNAL))
                        .child("SYSTEM / OVERVIEW"),
                )
                .child(
                    div()
                        .text_2xl()
                        .font_weight(gpui::FontWeight::BOLD)
                        .child(product.to_string()),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(MUTED))
                        .child(format!("{family}  ·  {platform}")),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .items_end()
                .gap_1()
                .text_xs()
                .child(div().text_color(rgb(MUTED)).child("FIRMWARE"))
                .child(div().text_color(rgb(TEXT)).child(bios.to_string())),
        )
}

fn safety_banner(status_message: &str) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_3()
        .px_4()
        .py_3()
        .bg(rgb(SURFACE_RAISED))
        .border_1()
        .border_color(rgb(CAUTION))
        .child(div().w(px(MARKER_SIZE)).h(px(MARKER_SIZE)).bg(rgb(CAUTION)))
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(CAUTION))
                        .child("SAFETY GATE / NO MUTATIONS"),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(TEXT))
                        .child(status_message.to_string()),
                ),
        )
}

fn capability_matrix(capabilities: &CapabilitySet) -> impl IntoElement {
    let rows = capabilities.features.iter().map(|(feature, capability)| {
        let (label, color) = availability_presentation(capability.availability);
        let detail = capability
            .detail
            .as_deref()
            .unwrap_or("No additional detail reported");
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_3()
            .px_3()
            .py_3()
            .border_b_1()
            .border_color(rgb(RULE))
            .child(div().w(px(MARKER_SIZE)).h(px(MARKER_SIZE)).bg(color))
            .child(
                div()
                    .w(px(FEATURE_COLUMN_WIDTH))
                    .flex_none()
                    .text_sm()
                    .child(feature.clone()),
            )
            .child(
                div()
                    .w(px(AVAILABILITY_COLUMN_WIDTH))
                    .flex_none()
                    .text_xs()
                    .text_color(color)
                    .child(label),
            )
            .child(
                div()
                    .flex_1()
                    .text_xs()
                    .text_color(rgb(MUTED))
                    .child(detail.to_string()),
            )
    });

    div()
        .flex()
        .flex_col()
        .min_h_0()
        .overflow_hidden()
        .border_1()
        .border_color(rgb(RULE))
        .child(
            div()
                .flex()
                .flex_row()
                .justify_between()
                .items_center()
                .px_4()
                .py_3()
                .bg(rgb(SURFACE_RAISED))
                .child(
                    div()
                        .text_sm()
                        .font_weight(gpui::FontWeight::BOLD)
                        .child("CAPABILITY MATRIX"),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(MUTED))
                        .child(format!("{} SIGNALS", capabilities.features.len())),
                ),
        )
        .child(div().flex().flex_col().children(rows).when(
            capabilities.features.is_empty(),
            |this| {
                this.child(
                    div()
                        .px_3()
                        .py_4()
                        .text_sm()
                        .text_color(rgb(UNAVAILABLE))
                        .child("No capability claims available; awaiting a platform HAL."),
                )
            },
        ))
}

fn telemetry_panel(snapshot: &DashboardSnapshot) -> impl IntoElement {
    let availability_count = snapshot
        .capabilities
        .features
        .values()
        .filter(|capability| capability.availability == Availability::Available)
        .count();
    let limited_count = snapshot
        .capabilities
        .features
        .values()
        .filter(|capability| capability.availability == Availability::Limited)
        .count();
    let unavailable_count = snapshot
        .capabilities
        .features
        .values()
        .filter(|capability| capability.availability == Availability::Unavailable)
        .count();

    div()
        .flex()
        .flex_col()
        .gap_3()
        .child(div().text_xs().text_color(rgb(SIGNAL)).child("TELEMETRY / READ PATH"))
        .child(
            div()
                .flex()
                .flex_row()
                .gap_3()
                .children([
                    metric("AVAILABLE", availability_count, SIGNAL),
                    metric("LIMITED", limited_count, CAUTION),
                    metric("UNAVAILABLE", unavailable_count, UNAVAILABLE),
                ]),
        )
        .child(
            div()
                .text_xs()
                .text_color(rgb(MUTED))
                .child("This shell never presents a disabled operation as executed. Hardware writes require an explicit controller dispatcher."),
        )
}

fn metric(label: &str, value: usize, color: u32) -> impl IntoElement {
    div()
        .flex_1()
        .flex()
        .flex_col()
        .gap_1()
        .p_4()
        .bg(rgb(SURFACE))
        .border_1()
        .border_color(rgb(RULE))
        .child(
            div()
                .text_2xl()
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(rgb(color))
                .child(value.to_string()),
        )
        .child(
            div()
                .text_xs()
                .text_color(rgb(MUTED))
                .child(label.to_string()),
        )
}

fn platform_name(platform: Platform) -> &'static str {
    match platform {
        Platform::Windows => "WINDOWS",
        Platform::Linux => "LINUX",
    }
}

fn availability_presentation(availability: Availability) -> (&'static str, gpui::Hsla) {
    match availability {
        Availability::Available => ("AVAILABLE", rgb(SIGNAL).into()),
        Availability::Limited => ("LIMITED", rgb(CAUTION).into()),
        Availability::Unavailable => ("UNAVAILABLE", rgb(UNAVAILABLE).into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lctrl_core::{CapabilitySet, HardwareInfo, LctrlError};

    struct ReadOnlyHal {
        capabilities: CapabilitySet,
    }

    impl Hal for ReadOnlyHal {
        fn platform(&self) -> Platform {
            self.capabilities.platform
        }

        fn hardware_info(&self) -> Result<HardwareInfo> {
            Ok(HardwareInfo {
                product_name: Some("Test Control Center".into()),
                family: Some("Test Family".into()),
                bios_version: Some("0.1".into()),
            })
        }

        fn capabilities(&self) -> Result<CapabilitySet> {
            Ok(self.capabilities.clone())
        }
    }

    #[test]
    fn snapshot_preserves_unavailable_capability_detail() {
        let mut capabilities = CapabilitySet::new(Platform::Linux);
        capabilities
            .record(
                "battery.threshold",
                Availability::Unavailable,
                Some("arbitrary percentage writes are unavailable".into()),
            )
            .expect("valid capability id");
        let snapshot =
            DashboardSnapshot::from_hal(&ReadOnlyHal { capabilities }).expect("read succeeds");
        let capability = snapshot
            .capabilities
            .get("battery.threshold")
            .expect("capability is retained");

        assert_eq!(capability.availability, Availability::Unavailable);
        assert_eq!(
            capability.detail.as_deref(),
            Some("arbitrary percentage writes are unavailable")
        );
        assert_ne!(capability.availability, Availability::Available);
    }

    #[test]
    fn from_hal_keeps_core_errors_strict() {
        struct FailingHal;
        impl Hal for FailingHal {
            fn platform(&self) -> Platform {
                Platform::Linux
            }
            fn hardware_info(&self) -> Result<HardwareInfo> {
                Err(LctrlError::Unsupported {
                    feature: "hardware identity".into(),
                })
            }
            fn capabilities(&self) -> Result<CapabilitySet> {
                panic!("capabilities must not be queried after hardware failure")
            }
        }

        let error =
            DashboardSnapshot::from_hal(&FailingHal).expect_err("core error must propagate");
        assert!(
            matches!(error, LctrlError::Unsupported { feature } if feature == "hardware identity")
        );
    }
}
