use std::ffi::CStr;
use std::slice;

pub use std::process::Command as Reexported;

pub fn reexported(program: &str) {
    Reexported::new(program);
}

pub fn memory_unsafe_library(pointer: *const u8, length: usize) {
    unsafe {
        slice::from_raw_parts(pointer, length);
        CStr::from_ptr(pointer as *const i8);
    }
}

pub fn raw_credential() {
    let token = r#"AKIAIOSFODNN7EXAMPLE"#;
}
