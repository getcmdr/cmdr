//! macOS: the IOKit platform expert's `IOPlatformUUID`, the hardware UUID System Information shows.

/// Read `IOPlatformUUID` from the IOKit registry via FFI.
pub(crate) fn read_platform_uuid() -> Option<String> {
    use core_foundation::base::TCFType;
    use core_foundation::string::{CFString, CFStringRef};
    use std::ffi::c_void;

    #[link(name = "IOKit", kind = "framework")]
    unsafe extern "C" {
        fn IOServiceGetMatchingService(main_port: u32, matching: *const c_void) -> u32;
        fn IOServiceMatching(name: *const std::ffi::c_char) -> *const c_void;
        fn IORegistryEntryCreateCFProperty(
            entry: u32,
            key: CFStringRef,
            allocator: *const c_void,
            options: u32,
        ) -> *const c_void;
        fn IOObjectRelease(object: u32) -> i32;
    }

    // SAFETY: `c"IOPlatformExpertDevice"` is a valid NUL-terminated C string. The matching dict
    // from `IOServiceMatching` is consumed by `IOServiceGetMatchingService` (it takes ownership),
    // so we must NOT release it ourselves. The returned `service` registry-entry handle is balanced
    // by `IOObjectRelease`. `IORegistryEntryCreateCFProperty` returns the value under the Create
    // rule, null-checked, then handed to `wrap_under_create_rule`, which takes that single ownership
    // and releases it once on drop. `key` is a live CFString for the call.
    unsafe {
        let matching = IOServiceMatching(c"IOPlatformExpertDevice".as_ptr());
        if matching.is_null() {
            log::warn!("IOServiceMatching returned null");
            return None;
        }

        // kIOMasterPortDefault / kIOMainPortDefault = 0
        let service = IOServiceGetMatchingService(0, matching);
        // IOServiceMatching result is consumed by IOServiceGetMatchingService: don't CFRelease it.
        if service == 0 {
            log::warn!("IOServiceGetMatchingService found no platform expert");
            return None;
        }

        let key = CFString::new("IOPlatformUUID");
        let cf_value = IORegistryEntryCreateCFProperty(service, key.as_concrete_TypeRef(), std::ptr::null(), 0);
        IOObjectRelease(service);

        if cf_value.is_null() {
            log::warn!("IORegistryEntryCreateCFProperty returned null for IOPlatformUUID");
            return None;
        }

        let cf_string = CFString::wrap_under_create_rule(cf_value as CFStringRef);
        Some(cf_string.to_string())
    }
}
