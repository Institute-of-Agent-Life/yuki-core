use crate::Result;
use std::ffi::{c_char, c_void};

#[repr(C)]
struct Span {
    data: *const c_char,
    size: usize,
}
impl Span {
    fn new(bytes: &[u8]) -> Self {
        Self {
            data: bytes.as_ptr().cast(),
            size: bytes.len(),
        }
    }
}
unsafe extern "C" {
    fn yuki_opencc_t2s(
        config: Span,
        dicts: *const Span,
        input: Span,
        context: *mut c_void,
        write: extern "C" fn(*mut c_void, *const c_char, usize),
    ) -> i32;
}
extern "C" fn output(ctx: *mut c_void, data: *const c_char, len: usize) {
    // OpenCC calls this synchronously, with a live std::string and our exclusive output buffer.
    unsafe {
        let result = &mut *ctx.cast::<Vec<u8>>();
        result.extend_from_slice(std::slice::from_raw_parts(data.cast(), len));
    }
}
macro_rules! resource {
    ($name:literal) => {
        include_bytes!(concat!(env!("YUKI_OPENCC_RESOURCES"), "/", $name))
    };
}
pub fn t2s(text: &str) -> Result<String> {
    let names = [
        Span::new(resource!("CJK_Compatibility_Ideographs.ocd2")),
        Span::new(resource!("TSPhrases.ocd2")),
        Span::new(resource!("TSCharactersExt.ocd2")),
        Span::new(resource!("TSCharacters.ocd2")),
    ];
    let mut bytes = Vec::<u8>::new();
    // The converter keeps only pointers into static embedded dictionary bytes. Neither input
    // nor result buffer outlives this call. C++ exceptions never cross the C ABI.
    let rc = unsafe {
        yuki_opencc_t2s(
            Span::new(resource!("t2s.json")),
            names.as_ptr(),
            Span::new(text.as_bytes()),
            (&mut bytes as *mut Vec<u8>).cast(),
            output,
        )
    };
    if rc != 0 {
        return Err("OpenCC 1.4.2 t2s conversion failed".into());
    }
    Ok(String::from_utf8(bytes)?)
}
