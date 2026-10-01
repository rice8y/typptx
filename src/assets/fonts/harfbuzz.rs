//! The bundled C++ bridge owns all HarfBuzz objects and returns an owned blob.
use std::ffi::{c_char, c_uint, c_void};

unsafe extern "C" {
    fn typptx_hb_instantiate(
        data: *const c_char,
        length: c_uint,
        index: c_uint,
        tags: *const c_uint,
        values: *const f32,
        count: c_uint,
    ) -> *mut c_void;
    fn typptx_hb_data(blob: *mut c_void, length: *mut c_uint) -> *const c_char;
    fn typptx_hb_destroy(blob: *mut c_void);
}

struct Blob(std::ptr::NonNull<c_void>);
impl Drop for Blob {
    fn drop(&mut self) {
        // This pointer is the owned reference returned by the bridge.
        unsafe { typptx_hb_destroy(self.0.as_ptr()) };
    }
}

pub(super) fn instantiate(data: &[u8], index: u32, axes: &[(u32, f32)]) -> Option<Vec<u8>> {
    let length = data.len().try_into().ok()?;
    let count = axes.len().try_into().ok()?;
    let (tags, values): (Vec<_>, Vec<_>) = axes.iter().copied().unzip();
    // All input slices live until the returned blob has been copied and dropped.
    // The bridge owns the blob reference; its contents never escape this borrow.
    let blob = Blob(std::ptr::NonNull::new(unsafe {
        typptx_hb_instantiate(
            data.as_ptr().cast(),
            length,
            index,
            tags.as_ptr(),
            values.as_ptr(),
            count,
        )
    })?);
    let mut length = 0;
    // HarfBuzz keeps these bytes valid until the owned blob is destroyed.
    let bytes = unsafe { typptx_hb_data(blob.0.as_ptr(), &mut length) };
    if bytes.is_null() || length == 0 {
        return None;
    }
    Some(unsafe { std::slice::from_raw_parts(bytes.cast(), length as usize) }.to_vec())
}

#[cfg(test)]
mod tests {
    #[test]
    fn invalid_fonts_fail_without_exposing_an_empty_blob() {
        for bytes in [&[][..], b"not a font"] {
            assert!(super::instantiate(bytes, 0, &[]).is_none());
        }
    }
}
