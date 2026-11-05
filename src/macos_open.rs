//! macOS: files opened from Finder (double-click, drag onto the Dock icon, `open file.katachi`)
//! arrive as `application:openURLs:` on the app delegate, which winit doesn't implement. Katachi
//! adds that method at runtime and queues the paths for the app to open.
//!
//! Quitting (Cmd Q, Dock > Quit, logging out) is the same: winit has no
//! `applicationShouldTerminate:`, so AppKit would quit without asking about unsaved work.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

static OPENED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// Paths macOS asked us to open since the last call.
pub fn take() -> Vec<PathBuf> {
    std::mem::take(&mut *OPENED.lock().unwrap())
}

/// Some tab has unsaved changes, so quitting has to go through the "Save changes?" questions.
pub static UNSAVED: AtomicBool = AtomicBool::new(false);
static QUIT: AtomicBool = AtomicBool::new(false);
/// To wake the app when macOS asks it to quit (no winit event arrives for that).
static CTX: OnceLock<egui::Context> = OnceLock::new();

pub fn set_context(ctx: &egui::Context) {
    let _ = CTX.set(ctx.clone());
}

/// Whether macOS asked to quit since the last call.
pub fn quit_requested() -> bool {
    QUIT.swap(false, Ordering::Relaxed)
}

#[cfg(target_os = "macos")]
pub fn install() {
    use objc2::runtime::{AnyClass, AnyObject, Sel};
    unsafe extern "C-unwind" fn open_urls(_this: *mut AnyObject, _cmd: Sel, _app: *mut AnyObject, urls: *mut AnyObject) {
        unsafe {
            let count: usize = objc2::msg_send![urls, count];
            for i in 0..count {
                let url: *mut AnyObject = objc2::msg_send![urls, objectAtIndex: i];
                let path: *mut AnyObject = objc2::msg_send![url, path];
                if path.is_null() {
                    continue;
                }
                let utf8: *const std::ffi::c_char = objc2::msg_send![path, UTF8String];
                if !utf8.is_null() {
                    let p = std::ffi::CStr::from_ptr(utf8).to_string_lossy().into_owned();
                    OPENED.lock().unwrap().push(PathBuf::from(p));
                }
            }
        }
    }
    /// No unsaved work: quit now. Otherwise cancel, and let the window close the usual way,
    /// which asks about each unsaved tab and then quits.
    unsafe extern "C-unwind" fn should_terminate(_this: *mut AnyObject, _cmd: Sel, _app: *mut AnyObject) -> usize {
        const NS_TERMINATE_CANCEL: usize = 0;
        const NS_TERMINATE_NOW: usize = 1;
        if !UNSAVED.load(Ordering::Relaxed) {
            return NS_TERMINATE_NOW;
        }
        QUIT.store(true, Ordering::Relaxed);
        if let Some(ctx) = CTX.get() {
            ctx.request_repaint();
        }
        NS_TERMINATE_CANCEL
    }
    // Added to NSObject (before the event loop exists) so winit's delegate inherits them in time
    // for files that launch the app; only AppKit calls these selectors, and only on the app delegate.
    let Some(cls) = AnyClass::get(c"NSObject") else { return };
    unsafe {
        let cls = cls as *const AnyClass as *mut AnyClass;
        let imp: objc2::runtime::Imp = std::mem::transmute(open_urls as unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject, *mut AnyObject));
        objc2::ffi::class_addMethod(cls, objc2::sel!(application:openURLs:), imp, c"v@:@@".as_ptr());
        let imp: objc2::runtime::Imp = std::mem::transmute(should_terminate as unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject) -> usize);
        objc2::ffi::class_addMethod(cls, objc2::sel!(applicationShouldTerminate:), imp, c"Q@:@".as_ptr());
    }
}

#[cfg(not(target_os = "macos"))]
pub fn install() {}
