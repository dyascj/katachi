//! macOS: files opened from Finder (double-click, drag onto the Dock icon, `open file.katachi`)
//! arrive as `application:openURLs:` on the app delegate, which winit doesn't implement. Katachi
//! adds that method at runtime and queues the paths for the app to open.

use std::path::PathBuf;
use std::sync::Mutex;

static OPENED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// Paths macOS asked us to open since the last call.
pub fn take() -> Vec<PathBuf> {
    std::mem::take(&mut *OPENED.lock().unwrap())
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
    // Added to NSObject (before the event loop exists) so winit's delegate inherits it in time for
    // files that launch the app; only AppKit calls this selector, and only on the app delegate.
    let Some(cls) = AnyClass::get(c"NSObject") else { return };
    unsafe {
        let imp: objc2::runtime::Imp = std::mem::transmute(open_urls as unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject, *mut AnyObject));
        objc2::ffi::class_addMethod(cls as *const AnyClass as *mut AnyClass, objc2::sel!(application:openURLs:), imp, c"v@:@@".as_ptr());
    }
}

#[cfg(not(target_os = "macos"))]
pub fn install() {}
