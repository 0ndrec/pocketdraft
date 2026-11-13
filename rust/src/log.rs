//! Diagnostics: the Android log (tag `Draftwright`, `adb logcat -s Draftwright`), standard
//! error elsewhere. A native library's standard error goes nowhere on Android.

/// Write one line to the log, `format!`-style.
macro_rules! note {
    ($($arg:tt)*) => {
        $crate::log::write(&format!($($arg)*))
    };
}
pub(crate) use note;

#[cfg(target_os = "android")]
pub fn write(message: &str) {
    use std::ffi::CString;
    use std::os::raw::{c_char, c_int};
    #[link(name = "log")]
    unsafe extern "C" {
        fn __android_log_write(priority: c_int, tag: *const c_char, text: *const c_char) -> c_int;
    }
    const ANDROID_LOG_INFO: c_int = 4;
    let Ok(text) = CString::new(message.replace('\0', " ")) else {
        return;
    };
    // SAFETY: both pointers are valid NUL-terminated strings for the duration of the call.
    unsafe {
        __android_log_write(ANDROID_LOG_INFO, c"Draftwright".as_ptr(), text.as_ptr());
    }
}

#[cfg(not(target_os = "android"))]
pub fn write(message: &str) {
    eprintln!("{message}");
}
