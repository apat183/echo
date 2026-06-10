//! macOS Accessibility (AX) access — reads the frontmost window's title and
//! manages the Accessibility permission. Title capture is best-effort: when the
//! permission isn't granted (or an app exposes no title) we return None and the
//! tracker falls back to app-level only.

#[cfg(target_os = "macos")]
mod imp {
    use core_foundation::base::{CFRelease, CFTypeRef, TCFType};
    use core_foundation::boolean::CFBoolean;
    use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
    use core_foundation::string::{CFString, CFStringRef};

    type AXUIElementRef = CFTypeRef;
    type AXError = i32;
    const AX_SUCCESS: AXError = 0;

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXUIElementCreateApplication(pid: i32) -> AXUIElementRef;
        fn AXUIElementCopyAttributeValue(
            element: AXUIElementRef,
            attribute: CFStringRef,
            value: *mut CFTypeRef,
        ) -> AXError;
        fn AXIsProcessTrusted() -> u8;
        fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> u8;
        static kAXTrustedCheckOptionPrompt: CFStringRef;
    }

    /// Does this process have Accessibility permission?
    pub fn is_trusted() -> bool {
        unsafe { AXIsProcessTrusted() != 0 }
    }

    /// Trigger the system "grant Accessibility" prompt (adds us to the list).
    pub fn prompt_trust() {
        unsafe {
            let key = CFString::wrap_under_get_rule(kAXTrustedCheckOptionPrompt);
            let opts = CFDictionary::from_CFType_pairs(&[(
                key.as_CFType(),
                CFBoolean::true_value().as_CFType(),
            )]);
            AXIsProcessTrustedWithOptions(opts.as_concrete_TypeRef());
        }
    }

    /// Title of the focused window of the app with the given pid, if available.
    pub fn focused_window_title(pid: i32) -> Option<String> {
        unsafe {
            let app_el = AXUIElementCreateApplication(pid);
            if app_el.is_null() {
                return None;
            }
            let title = copy_string_attr(app_el, "AXFocusedWindow", "AXTitle");
            CFRelease(app_el);
            title
        }
    }

    /// Follow `element.<window_attr>.<string_attr>` and return it as a String.
    unsafe fn copy_string_attr(
        element: AXUIElementRef,
        window_attr: &str,
        string_attr: &str,
    ) -> Option<String> {
        let win_key = CFString::new(window_attr);
        let mut win_ref: CFTypeRef = std::ptr::null();
        if AXUIElementCopyAttributeValue(element, win_key.as_concrete_TypeRef(), &mut win_ref)
            != AX_SUCCESS
            || win_ref.is_null()
        {
            return None;
        }

        let str_key = CFString::new(string_attr);
        let mut str_ref: CFTypeRef = std::ptr::null();
        let err =
            AXUIElementCopyAttributeValue(win_ref, str_key.as_concrete_TypeRef(), &mut str_ref);
        CFRelease(win_ref);
        if err != AX_SUCCESS || str_ref.is_null() {
            return None;
        }

        let cf = CFString::wrap_under_create_rule(str_ref as CFStringRef);
        let s = cf.to_string();
        let trimmed = s.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    pub fn is_trusted() -> bool {
        false
    }
    pub fn prompt_trust() {}
    pub fn focused_window_title(_pid: i32) -> Option<String> {
        None
    }
}

pub use imp::*;
