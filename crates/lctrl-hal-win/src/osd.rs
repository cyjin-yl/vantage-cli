//! Native on-screen display: a transient, centered, always-on-top GDI overlay.
//!
//! The overlay is a real top-level window (layered, click-through, topmost)
//! painted with GDI. It requires the active interactive desktop session; a
//! daemon running outside that session reports the gate honestly rather than
//! claiming it painted anything.
//!
//! The window is created and painted on demand; it stays up until `hide()`
//! destroys it. (No separate auto-hide thread: an OSD shown from an
//! interactive session is dismissed by the user or an explicit `hide`.)

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex};

use lctrl_core::Result;
use lctrl_hal::{OsdControl, OsdOutcome};
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
    Graphics::Gdi::{
        BeginPaint, CreateFontW, DT_CENTER, DT_WORDBREAK, DrawTextW, EndPaint, GetStockObject,
        PAINTSTRUCT, SelectObject, SetBkMode, SetTextColor, TRANSPARENT,
    },
    System::LibraryLoader::GetModuleHandleW,
    UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, GetSystemMetrics,
        RegisterClassExW, SetLayeredWindowAttributes, SetWindowPos, SM_CXSCREEN, SM_CYSCREEN,
        WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOPMOST, WS_EX_TRANSPARENT,
        WS_CLIPSIBLINGS, WS_POPUP, WS_VISIBLE, WM_DESTROY, WM_PAINT,
    },
};

/// The window class holds raw pointers valid for the process lifetime.
struct StaticClass(WNDCLASSEXW);
unsafe impl Send for StaticClass {}
unsafe impl Sync for StaticClass {}

static CLS: LazyLock<StaticClass> = LazyLock::new(|| {
    let hinstance = unsafe { GetModuleHandleW(std::ptr::null()) };
    let cls = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: 0,
        lpfnWndProc: Some(osd_wndproc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: hinstance,
        hIcon: std::ptr::null_mut(),
        hCursor: unsafe { GetStockObject(3) } as *mut _, // OIC_ARROW
        hbrBackground: std::ptr::null_mut(),
        lpszMenuName: std::ptr::null(),
        lpszClassName: b"vantage_osd_class\0".as_ptr() as *const _,
        hIconSm: std::ptr::null_mut(),
    };
    unsafe {
        RegisterClassExW(&cls);
    }
    StaticClass(cls)
});

/// A shared window handle (raw pointer) wrapped for cross-thread safety.
struct StoredHwnd(HWND);
unsafe impl Send for StoredHwnd {}
unsafe impl Sync for StoredHwnd {}

static CURRENT: LazyLock<Mutex<Option<StoredHwnd>>> = LazyLock::new(|| Mutex::new(None));
static VISIBLE: AtomicBool = AtomicBool::new(false);
static OSD_TEXT: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// A transient GDI on-screen overlay.
#[derive(Default)]
pub struct NativeOsd;

impl NativeOsd {
    pub fn new() -> Self {
        Self
    }
}

impl OsdControl for NativeOsd {
    fn show(&self, lines: &[&str], dry_run: bool) -> Result<OsdOutcome> {
        let text: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
        if dry_run {
            return Ok(OsdOutcome {
                state: "dry_run".into(),
                lines: text,
                detail: "no window created during dry-run".into(),
            });
        }
        *OSD_TEXT.lock().unwrap() = text.clone();
        match show_overlay() {
            Ok(()) => {
                VISIBLE.store(true, Ordering::Release);
                Ok(OsdOutcome {
                    state: "painted".into(),
                    lines: text,
                    detail: "topmost overlay shown; dismissed by hide or user".into(),
                })
            }
            Err(detail) => {
                VISIBLE.store(false, Ordering::Release);
                Ok(OsdOutcome {
                    state: "gated".into(),
                    lines: text,
                    detail,
                })
            }
        }
    }

    fn hide(&self) -> Result<bool> {
        let was = VISIBLE.swap(false, Ordering::AcqRel);
        if was {
            if let Some(h) = CURRENT.lock().unwrap().take() {
                unsafe {
                    DestroyWindow(h.0);
                }
            }
        }
        Ok(was)
    }

    fn visible(&self) -> bool {
        VISIBLE.load(Ordering::Acquire)
    }
}

fn show_overlay() -> std::result::Result<(), String> {
    unsafe {
        let sw = GetSystemMetrics(SM_CXSCREEN);
        let sh = GetSystemMetrics(SM_CYSCREEN);
        if sw <= 0 || sh <= 0 {
            return Err("no interactive display surface is attached to this session".into());
        }
        let width = (sw / 100).max(200);
        let height = (sh / 20).max(80);
        let x = (sw - width) / 2;
        let y = (sh - height) / 2;

        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE,
            CLS.0.lpszClassName,
            b"vantage-osd\0".as_ptr() as *const _,
            WS_POPUP | WS_CLIPSIBLINGS | WS_VISIBLE,
            x,
            y,
            width,
            height,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            CLS.0.hInstance,
            std::ptr::null_mut(),
        );
        if hwnd.is_null() {
            return Err("CreateWindowExW failed; this service is not attached to the active \
                        desktop session"
                .into());
        }
        SetBkMode(hwnd, TRANSPARENT as i32);
        SetTextColor(hwnd, 0x00E6E6E6); // light gray text
        SetLayeredWindowAttributes(hwnd, 0, 0, 0);
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            x,
            y,
            width,
            height,
            0, // no SWP flags: reposition and show
        );
        *CURRENT.lock().unwrap() = Some(StoredHwnd(hwnd));
        Ok(())
    }
}

unsafe extern "system" fn osd_wndproc(
    hwnd: HWND,
    msg: u32,
    _wparam: WPARAM,
    _lparam: LPARAM,
) -> LRESULT {
    unsafe {
        match msg {
            WM_PAINT => {
                let mut ps: PAINTSTRUCT = std::mem::zeroed();
                let _ = BeginPaint(hwnd, &mut ps);
                let text = OSD_TEXT.lock().unwrap();
                if !text.is_empty() {
                    let joined: Vec<u16> =
                        text.join("\n").encode_utf16().chain(std::iter::once(0)).collect();
                    let mut rect = RECT {
                        left: 4,
                        top: 4,
                        right: 4,
                        bottom: 4,
                    };
                    GetClientRect(hwnd, &mut rect);
                    let font = CreateFontW(
                        -30,
                        0,
                        0,
                        0,
                        700,
                        0,
                        0,
                        0,
                        0,
                        0,
                        0,
                        0,
                        0,
                        b"Consolas\0".as_ptr() as *const _,
                    );
                    SelectObject(hwnd, font);
                    DrawTextW(
                        hwnd,
                        joined.as_ptr(),
                        joined.len() as i32 - 1,
                        &mut rect,
                        DT_CENTER | DT_WORDBREAK,
                    );
                }
                EndPaint(hwnd, &ps);
                0
            }
            WM_DESTROY => {
                *CURRENT.lock().unwrap() = None;
                VISIBLE.store(false, Ordering::Release);
                0
            }
            _ => DefWindowProcW(hwnd, msg, _wparam, _lparam),
        }
    }
}
