use crate::AvifData as AvifDataRust;

/// Result of parsing an AVIF file. Contains AV1-compressed data.
#[allow(bad_style)]
#[repr(C)]
pub struct avif_data_t {
    /// AV1 data for color channels
    pub primary_data: *const u8,
    pub primary_size: usize,
    /// AV1 data for alpha channel (may be NULL if the image is fully opaque)
    pub alpha_data: *const u8,
    pub alpha_size: usize,
    /// 0 = normal, uncorrelated alpha channel
    /// 1 = premultiplied alpha. You must divide RGB values by alpha.
    ///
    /// ```c
    /// if (a != 0) {r = r * 255 / a}
    /// ```
    pub premultiplied_alpha: u8,
    _rusty_handle: *mut AvifDataRust,
}

/// Parse AVIF image file and return results. Returns `NULL` if the file can't be parsed.
///
/// Call `avif_data_free` on the result when done.
#[no_mangle]
pub unsafe extern "C" fn avif_parse(bytes: *const u8, bytes_len: usize) -> *const avif_data_t {
    if bytes.is_null() || bytes_len == 0 {
        return std::ptr::null();
    }
    let mut data = std::slice::from_raw_parts(bytes, bytes_len);
    match crate::read_avif(&mut data) {
        Ok(data) => Box::into_raw(Box::new(avif_data_t {
            primary_data: data.primary_item.as_ptr(),
            primary_size: data.primary_item.len(),
            alpha_data: data
                .alpha_item
                .as_ref()
                .map_or(std::ptr::null(), |a| a.as_ptr()),
            alpha_size: data.alpha_item.as_ref().map_or(0, |a| a.len()),
            premultiplied_alpha: data.premultiplied_alpha as u8,
            _rusty_handle: Box::into_raw(Box::new(data)),
        })),
        Err(_) => std::ptr::null(),
    }
}

/// Free all data related to `avif_data_t`
#[no_mangle]
pub unsafe extern "C" fn avif_data_free(data: *const avif_data_t) {
    if data.is_null() {
        return;
    }
    let _ = Box::from_raw((*data)._rusty_handle);
    let _ = Box::from_raw(data as *mut avif_data_t);
}
