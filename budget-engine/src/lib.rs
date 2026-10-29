//! Raw UTF-8 JSON ABI. Result storage belongs to the instance until its next request.
pub use budget_core::sharing::request_json;
#[cfg(target_arch = "wasm32")]
mod raw {
    use std::sync::Mutex;
    static RESULT: Mutex<Vec<u8>> = Mutex::new(Vec::new());
    #[no_mangle]
    pub extern "C" fn alloc(len: usize) -> *mut u8 {
        if len > budget_core::sharing::MAX_REQUEST_BYTES {
            return std::ptr::null_mut();
        }
        Box::into_raw(vec![0u8; len].into_boxed_slice()) as *mut u8
    }
    /// Caller must supply the exact live allocation returned by alloc.
    #[no_mangle]
    pub unsafe extern "C" fn dealloc(ptr: *mut u8, len: usize) {
        if !ptr.is_null() {
            drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr, len)))
        }
    }
    /// Caller must supply len initialized bytes in its live input allocation.
    #[no_mangle]
    pub unsafe extern "C" fn request(ptr: *const u8, len: usize) {
        let response = if ptr.is_null() || len > budget_core::sharing::MAX_REQUEST_BYTES {
            "{\"ok\":false,\"error\":\"Invalid input allocation.\"}".to_string()
        } else {
            match std::str::from_utf8(std::slice::from_raw_parts(ptr, len)) {
                Ok(input) => super::request_json(input),
                Err(_) => "{\"ok\":false,\"error\":\"Input must be UTF-8.\"}".to_string(),
            }
        };
        *RESULT.lock().unwrap() = response.into_bytes();
    }
    #[no_mangle]
    pub extern "C" fn result_ptr() -> *const u8 {
        RESULT.lock().unwrap().as_ptr()
    }
    #[no_mangle]
    pub extern "C" fn result_len() -> usize {
        RESULT.lock().unwrap().len()
    }
}
