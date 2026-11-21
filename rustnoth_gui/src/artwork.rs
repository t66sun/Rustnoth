//! Decode original artwork before uploading pixels to the GPU.
pub(crate) mod scaled;
use image::RgbaImage;
use macroquad::texture::Image;
use std::error::Error;

pub(super) fn rgba(bytes: &[u8]) -> Result<RgbaImage, Box<dyn Error>> {
    #[cfg(windows)]
    if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP")
        && let Some(result) = windows::decode(bytes)
    {
        return result;
    }
    Ok(image::load_from_memory(bytes)?.into_rgba8())
}

pub(super) fn image(bytes: &[u8]) -> Result<Image, Box<dyn Error>> {
    let decoded = rgba(bytes)?;
    Ok(Image {
        width: decoded.width().try_into()?,
        height: decoded.height().try_into()?,
        bytes: decoded.into_raw(),
    })
}

#[cfg(windows)]
mod windows {
    use super::*;
    use std::{ffi::{c_char, c_void}, os::windows::ffi::OsStrExt, path::Path, ptr};

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn LoadLibraryExW(path: *const u16, file: *mut c_void, flags: u32) -> *mut c_void;
        fn GetProcAddress(library: *mut c_void, name: *const c_char) -> *mut c_void;
        fn FreeLibrary(library: *mut c_void) -> i32;
    }

    struct Library(*mut c_void);
    impl Drop for Library {
        fn drop(&mut self) {
            unsafe { FreeLibrary(self.0); }
        }
    }

    type Info = unsafe extern "C" fn(*const u8, usize, *mut i32, *mut i32) -> i32;
    type Decode = unsafe extern "C" fn(*const u8, usize, *mut u8, usize, i32) -> *mut u8;
    struct Decoder {
        _library: Library,
        info: Info,
        decode: Decode,
    }

    impl Decoder {
        fn from_install(root: &Path) -> Option<Self> {
            // Resolve the codec from the installed game. Asset roots may point
            // to a checkout or an add-on, so are never executable search paths.
            if !root.is_absolute() || !root.join("wesnoth.exe").is_file() { return None; }
            let path: Vec<_> = root.join("libwebp-7.dll").as_os_str().encode_wide().chain(Some(0)).collect();
            // LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32
            let handle = unsafe { LoadLibraryExW(path.as_ptr(), ptr::null_mut(), 0x100 | 0x800) };
            if handle.is_null() { return None; }
            let library = Library(handle);
            let info = unsafe { GetProcAddress(handle, c"WebPGetInfo".as_ptr()) };
            let decode = unsafe { GetProcAddress(handle, c"WebPDecodeRGBAInto".as_ptr()) };
            if info.is_null() || decode.is_null() { return None; }
            // These two functions have stable C signatures in libwebp ABI 7.
            Some(Self {
                _library: library,
                info: unsafe { std::mem::transmute::<*mut c_void, Info>(info) },
                decode: unsafe { std::mem::transmute::<*mut c_void, Decode>(decode) },
            })
        }

        fn rgba(&self, bytes: &[u8]) -> Result<RgbaImage, Box<dyn Error>> {
            let (mut width, mut height) = (0, 0);
            if unsafe { (self.info)(bytes.as_ptr(), bytes.len(), &mut width, &mut height) } == 0 {
                return Err("Invalid WebP header".into());
            }
            let (w, h) = (u32::try_from(width)?, u32::try_from(height)?);
            image::io::Limits::default().reserve_buffer(w, h, image::ColorType::Rgba8)?;
            let mut image = RgbaImage::new(w, h);
            // libwebp checks the supplied stride and buffer size before writing.
            let result = unsafe { (self.decode)(bytes.as_ptr(), bytes.len(),
                image.as_mut_ptr(), image.len(), width * 4) };
            if result.is_null() { return Err("Invalid WebP pixel data".into()); }
            Ok(image)
        }
    }

    pub(super) fn decode(bytes: &[u8]) -> Option<Result<RgbaImage, Box<dyn Error>>> {
        thread_local! {
            static DECODER: Option<Decoder> = crate::desktop::steam_game_roots()
                .iter().find_map(|root| Decoder::from_install(root));
        }
        DECODER.with(|decoder| decoder.as_ref().map(|codec| codec.rgba(bytes)))
    }
}
