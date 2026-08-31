//! Windows-specific glue: console re-attach (so running from a terminal still
//! shows progress, while double-clicking shows no console), file drag & drop via
//! a window-procedure subclass (three-d does not forward winit's drop events),
//! window title updates through the raw window handle, native error dialogs, and
//! monitor metrics for a sensible starting window size.

use std::cell::RefCell;
use std::ffi::{c_void, OsString};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::{Mutex, OnceLock};

type Hwnd = *mut c_void;
type WndProc = unsafe extern "system" fn(Hwnd, u32, usize, isize) -> isize;

const WM_DROPFILES: u32 = 0x0233;
const GWLP_WNDPROC: i32 = -4;
const SM_CXSCREEN: i32 = 0;
const SM_CYSCREEN: i32 = 1;
const MB_OK: u32 = 0;
const MB_ICONERROR: u32 = 0x10;
const ATTACH_PARENT_PROCESS: u32 = 0xFFFF_FFFF;

static MAIN_HWND: AtomicIsize = AtomicIsize::new(0);
static DROPPED: OnceLock<Mutex<Vec<PathBuf>>> = OnceLock::new();

// the subclassed window's thread runs the render loop and therefore also the
// subclass proc, so a thread-local is the right home for the original wndproc
thread_local! {
    static PREV_PROC: std::cell::Cell<Option<WndProc>> = const { std::cell::Cell::new(None) };
}

fn queue() -> &'static Mutex<Vec<PathBuf>> {
    DROPPED.get_or_init(|| Mutex::new(Vec::new()))
}

extern "system" {
    fn AttachConsole(dwProcessId: u32) -> i32;
    fn MessageBoxW(hwnd: Hwnd, text: *const u16, caption: *const u16, utype: u32) -> i32;
    fn SetWindowTextW(hwnd: Hwnd, string: *const u16) -> i32;
    fn GetSystemMetrics(nIndex: i32) -> i32;
    fn SetWindowLongPtrW(hwnd: Hwnd, index: i32, newlong: isize) -> isize;
    fn CallWindowProcW(prev: WndProc, hwnd: Hwnd, msg: u32, w: usize, l: isize) -> isize;
    fn DragQueryFileW(hdrop: *mut c_void, ifile: u32, buf: *mut u16, buflen: u32) -> u32;
    fn DragFinish(hdrop: *mut c_void);
    fn DragAcceptFiles(hwnd: Hwnd, accept: i32);
    fn RevokeDragDrop(hwnd: Hwnd) -> i32;
    fn EnumWindows(cb: EnumProc, lparam: LPARAM) -> i32;
    fn GetWindowThreadProcessId(hwnd: Hwnd, pid: *mut u32) -> u32;
    fn IsWindowVisible(hwnd: Hwnd) -> i32;
    fn GetWindowTextW(hwnd: Hwnd, buf: *mut u16, max: i32) -> i32;
    fn GetCurrentProcessId() -> u32;
}
type LPARAM = isize;
type EnumProc = unsafe extern "system" fn(Hwnd, LPARAM) -> i32;

/// Locate this process's viewer window (three-d does not expose the native
/// handle, so we look it up the way any outsider would). Polls briefly because
/// winit creates it asynchronously on some backends.
fn find_main_hwnd() -> Option<isize> {
    thread_local! {
        static FOUND: RefCell<Option<isize>> = const { RefCell::new(None) };
        static PREFIX: Vec<u16> = "simple-3d-viewer".encode_utf16().collect();
    }
    unsafe extern "system" fn cb(hwnd: Hwnd, _l: LPARAM) -> i32 {
        let pid = GetCurrentProcessId();
        let mut wpid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut wpid);
        if wpid != pid || IsWindowVisible(hwnd) == 0 {
            return 1;
        }
        let mut buf = [0u16; 64];
        let len = GetWindowTextW(hwnd, buf.as_mut_ptr(), 64);
        if len <= 0 {
            return 1;
        }
        let got = &buf[..len as usize];
        PREFIX.with(|p| {
            let p = &**p;
            let take = p.len().min(got.len());
            if got[..take] == p[..take] {
                FOUND.with(|f| *f.borrow_mut() = Some(hwnd as isize));
                return 0;
            }
            1
        })
    }
    for _ in 0..100 {
        FOUND.with(|f| f.borrow_mut().take());
        unsafe {
            EnumWindows(cb, 0);
        }
        if let Some(h) = FOUND.with(|f| *f.borrow()) {
            return Some(h);
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    None
}

/// The viewer's drop target. winit installs an OLE `IDropTarget` on the window
/// (which swallows Explorer drags and never posts `WM_DROPFILES`), so we revoke
/// that registration and fall back to the classic `DragAcceptFiles` mechanism,
/// then subclass the window procedure to receive `WM_DROPFILES`.
pub struct DropTarget {
    hwnd: isize,
}

impl DropTarget {
    pub fn register() -> Option<DropTarget> {
        let hwnd = find_main_hwnd()?;
        MAIN_HWND.store(hwnd, Ordering::SeqCst);
        let hwnd_ptr = hwnd as *mut c_void;
        unsafe {
            // drop winit's OLE target so Explorer uses the legacy WM_DROPFILES path
            RevokeDragDrop(hwnd_ptr);
            let prev = SetWindowLongPtrW(hwnd_ptr, GWLP_WNDPROC, subclass_proc as *const c_void as isize);
            PREV_PROC.with(|p| {
                p.set(if prev == 0 {
                    None
                } else {
                    Some(std::mem::transmute::<isize, WndProc>(prev))
                })
            });
            DragAcceptFiles(hwnd_ptr, 1);
        }
        Some(DropTarget { hwnd })
    }

    /// Take (and clear) the paths dropped onto the window since the last call.
    /// Must only be called from the window's thread (the render loop), which is
    /// the same thread the subclass proc runs on — it can never interleave with
    /// the borrow of the queue below.
    pub fn take_drops(&self) -> Vec<PathBuf> {
        let mut q = match queue().lock() {
            Ok(q) => q,
            Err(poisoned) => poisoned.into_inner(),
        };
        std::mem::take(&mut *q)
    }
}

impl Drop for DropTarget {
    fn drop(&mut self) {
        unsafe {
            PREV_PROC.with(|p| {
                if let Some(prev) = p.get() {
                    SetWindowLongPtrW(self.hwnd as *mut c_void, GWLP_WNDPROC, prev as *const c_void as isize);
                }
            });
        }
    }
}

unsafe extern "system" fn subclass_proc(hwnd: Hwnd, msg: u32, wparam: usize, lparam: isize) -> isize {
    if msg == WM_DROPFILES {
        let hdrop = wparam as *mut c_void;
        let count = DragQueryFileW(hdrop, u32::MAX, std::ptr::null_mut(), 0);
        let mut paths = Vec::new();
        for i in 0..count {
            let len = DragQueryFileW(hdrop, i, std::ptr::null_mut(), 0);
            let mut buf = vec![0u16; (len + 1) as usize];
            let written = DragQueryFileW(hdrop, i, buf.as_mut_ptr(), len + 1);
            buf.truncate(written as usize);
            paths.push(PathBuf::from(OsString::from_wide(&buf)));
        }
        DragFinish(hdrop);
        let mut q = match queue().lock() {
            Ok(q) => q,
            Err(poisoned) => poisoned.into_inner(),
        };
        q.extend(paths);
        return 0; // handled; winit would only discard it anyway
    }
    match PREV_PROC.with(|p| p.get()) {
        Some(prev) => CallWindowProcW(prev, hwnd, msg, wparam, lparam),
        None => 0,
    }
}

/// When launched from a terminal, re-attach to its console so progress lines and
/// errors appear there; double-click launches have no parent console and stay
/// fully console-free. The attached console window (if any) is the *parent's*,
/// so it must never be shown or hidden from here.
pub fn attach_console() {
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

pub fn set_title(text: &str) {
    let hwnd = MAIN_HWND.load(Ordering::SeqCst);
    if hwnd == 0 {
        return;
    }
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        SetWindowTextW(hwnd as *mut c_void, wide.as_ptr());
    }
}

pub fn message_box(msg: &str) {
    let text: Vec<u16> = msg.encode_utf16().chain(std::iter::once(0)).collect();
    let cap: Vec<u16> = "simple-3d-viewer".encode_utf16().chain(std::iter::once(0)).collect();
    // no parent: this is also called after the window is gone (startup errors)
    unsafe {
        MessageBoxW(std::ptr::null_mut(), text.as_ptr(), cap.as_ptr(), MB_OK | MB_ICONERROR);
    }
}

/// (width, height) in physical pixels for a comfortable starting window on the
/// primary monitor.
pub fn initial_size() -> (u32, u32) {
    unsafe {
        let w = GetSystemMetrics(SM_CXSCREEN);
        let h = GetSystemMetrics(SM_CYSCREEN);
        if w > 200 && h > 200 {
            ((w as u32 * 4) / 5, (h as u32 * 4) / 5)
        } else {
            (1600, 900)
        }
    }
}

/// Seconds since the file was last written, or None if it cannot be opened.
/// Uses CreateFileW directly (not metadata) so we succeed on files another
/// process holds with only its own sharing flags set.
pub fn file_modified(path: &Path) -> Option<i64> {
    extern "system" {
        fn CreateFileW(
            name: *const u16,
            access: u32,
            share: u32,
            sa: *mut c_void,
            disp: u32,
            flags: u32,
            template: Hwnd,
        ) -> *mut c_void;
        fn GetFileTime(
            handle: *mut c_void,
            creation: *mut u64,
            access_t: *mut u64,
            write_t: *mut u64,
        ) -> i32;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }
    const FILE_READ_ATTRIBUTES: u32 = 0x80;
    const FILE_SHARE_ALL: u32 = 0x07;
    const OPEN_EXISTING: u32 = 3;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    const INVALID_HANDLE: *mut c_void = -1isize as *mut c_void;
    // FILETIME: 100ns ticks since 1601-01-01
    const WIN_UNIX_DIFF: u64 = 116_444_736_000_000_000;

    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    unsafe {
        let h = CreateFileW(
            wide.as_ptr(),
            FILE_READ_ATTRIBUTES,
            FILE_SHARE_ALL,
            std::ptr::null_mut(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            std::ptr::null_mut(),
        );
        if h == INVALID_HANDLE || h.is_null() {
            return None;
        }
        let mut ft = 0u64;
        let ok = GetFileTime(h, std::ptr::null_mut(), std::ptr::null_mut(), &mut ft);
        CloseHandle(h);
        if ok == 0 {
            return None;
        }
        if ft < WIN_UNIX_DIFF {
            return Some(0);
        }
        Some(((ft - WIN_UNIX_DIFF) / 10_000_000) as i64)
    }
}
