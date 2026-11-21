//! Windows installations supply the Pango, Cairo and FreeType stack used by Wesnoth.
use std::{ffi::{CString, c_char, c_void}, os::windows::ffi::OsStrExt, path::Path, ptr};

type Handle = *mut c_void;
#[link(name = "kernel32")]
unsafe extern "system" {
    fn LoadLibraryExW(file: *const u16, reserved: Handle, flags: u32) -> Handle;
    fn GetProcAddress(module: Handle, name: *const c_char) -> Handle;
    fn FreeLibrary(module: Handle) -> i32;
}

struct Library(Handle);
impl Library {
    fn open(dir: &Path, name: &str) -> Option<Self> {
        let path: Vec<_> = dir.join(name).as_os_str().encode_wide().chain([0]).collect();
        // Search only this installation and the system DLL directory for dependencies.
        let module = unsafe { LoadLibraryExW(path.as_ptr(), ptr::null_mut(), 0x100 | 0x800) };
        if module.is_null() { None } else { Some(Self(module)) }
    }
}

impl Drop for Library {
    fn drop(&mut self) { unsafe { FreeLibrary(self.0); } }
}

// Keep a reference to each library for as long as its exported functions are callable.
macro_rules! api {
    ($($lib:ident = $dll:literal { $(fn $name:ident($($arg:ty),*) $(-> $ret:ty)?;)* })*) => {
        #[allow(non_snake_case)] // Match Fontconfig's exported symbol names.
        struct Api {
            $( $($name: unsafe extern "C" fn($($arg),*) $(-> $ret)?,)* )*
            _libraries: Vec<Library>,
        }
        impl Api {
            fn load(dir: &Path) -> Option<Self> {
                $(let $lib = Library::open(dir, $dll)?;)*
                Some(Self {
                    $($($name: {
                        let addr = unsafe { GetProcAddress($lib.0, concat!(stringify!($name), "\0").as_ptr().cast()) };
                        if addr.is_null() { return None; }
                        // Each signature below follows the named function's public C ABI.
                        unsafe { std::mem::transmute::<Handle, unsafe extern "C" fn($($arg),*) $(-> $ret)?>(addr) }
                    },)*)*
                    _libraries: vec![$($lib),*],
                })
            }
        }
    };
}
api! {
    fc = "libfontconfig-1.dll" {
        fn FcConfigCreate() -> Handle;
        fn FcConfigParseAndLoad(Handle, *const u8, i32) -> i32;
        fn FcConfigAppFontAddDir(Handle, *const u8) -> i32;
        fn FcConfigDestroy(Handle);
    }
    ft = "libpangoft2-1.0-0.dll" {
        fn pango_fc_font_map_set_config(Handle, Handle);
    }
    glib = "libgobject-2.0-0.dll" {
        fn g_object_unref(Handle);
    }
    pango = "libpango-1.0-0.dll" {
        fn pango_font_map_create_context(Handle) -> Handle;
        fn pango_layout_new(Handle) -> Handle;
        fn pango_layout_set_width(Handle, i32);
        fn pango_layout_set_wrap(Handle, i32);
        fn pango_layout_set_alignment(Handle, i32);
        fn pango_layout_set_line_spacing(Handle, f32);
        fn pango_layout_set_markup(Handle, *const c_char, i32);
        fn pango_layout_set_text(Handle, *const c_char, i32);
        fn pango_layout_set_attributes(Handle, Handle);
        fn pango_font_description_new() -> Handle;
        fn pango_font_description_set_family(Handle, *const c_char);
        fn pango_font_description_set_size(Handle, i32);
        fn pango_font_description_free(Handle);
        fn pango_layout_set_font_description(Handle, Handle);
        fn pango_layout_get_pixel_size(Handle, *mut i32, *mut i32);
    }
    pc = "libpangocairo-1.0-0.dll" {
        fn pango_cairo_font_map_new_for_font_type(i32) -> Handle;
        fn pango_cairo_context_set_resolution(Handle, f64);
        fn pango_cairo_context_set_font_options(Handle, Handle);
        fn pango_cairo_show_layout(Handle, Handle);
    }
    cairo = "libcairo-2.dll" {
        fn cairo_font_options_create() -> Handle;
        fn cairo_font_options_destroy(Handle);
        fn cairo_font_options_set_hint_style(Handle, i32);
        fn cairo_font_options_set_hint_metrics(Handle, i32);
        fn cairo_image_surface_create_for_data(*mut u8, i32, i32, i32, i32) -> Handle;
        fn cairo_surface_destroy(Handle);
        fn cairo_surface_flush(Handle);
        fn cairo_create(Handle) -> Handle;
        fn cairo_set_source_rgba(Handle, f64, f64, f64, f64);
        fn cairo_destroy(Handle);
    }
}

pub(super) struct Text {
    api: Api,
    layout: Handle,
    map: Handle,
}

impl Drop for Text {
    fn drop(&mut self) {
        unsafe {
            (self.api.g_object_unref)(self.layout);
            (self.api.g_object_unref)(self.map);
        }
    }
}

impl Text {
    pub(super) fn load(assets: &Path, family: &str) -> Option<Self> {
        // Asset paths supplied on the command line are data only. Resolve DLLs through Steam.
        let api = crate::desktop::steam_game_roots().iter().find_map(|dir| Api::load(dir))?;
        let fonts = assets.join("fonts");
        let font_dir = CString::new(fonts.to_str()?).ok()?;
        let font_conf = CString::new(fonts.join("fonts.conf").to_str()?).ok()?;
        let family = CString::new(family).ok()?;
        unsafe {
            let config = (api.FcConfigCreate)();
            if config.is_null() { return None; }
            let loaded = (api.FcConfigParseAndLoad)(config, font_conf.as_ptr().cast(), 0) != 0
                && (api.FcConfigAppFontAddDir)(config, font_dir.as_ptr().cast()) != 0;
            // Use FreeType explicitly, as Wesnoth does on Windows.
            let map = if loaded { (api.pango_cairo_font_map_new_for_font_type)(1) } else { ptr::null_mut() };
            if !map.is_null() { (api.pango_fc_font_map_set_config)(map, config); }
            (api.FcConfigDestroy)(config);
            if map.is_null() { return None; }
            let ctx = (api.pango_font_map_create_context)(map);
            (api.pango_cairo_context_set_resolution)(ctx, 72.);
            let options = (api.cairo_font_options_create)();
            (api.cairo_font_options_set_hint_style)(options, 4); // CAIRO_HINT_STYLE_FULL
            (api.cairo_font_options_set_hint_metrics)(options, 2); // CAIRO_HINT_METRICS_ON
            (api.pango_cairo_context_set_font_options)(ctx, options);
            (api.cairo_font_options_destroy)(options);
            let layout = (api.pango_layout_new)(ctx);
            (api.g_object_unref)(ctx);
            (api.pango_layout_set_wrap)(layout, 2); // PANGO_WRAP_WORD_CHAR
            (api.pango_layout_set_line_spacing)(layout, 1.3);
            let font = (api.pango_font_description_new)();
            (api.pango_font_description_set_family)(font, family.as_ptr());
            (api.pango_font_description_set_size)(font, 17 * 1024);
            (api.pango_layout_set_font_description)(layout, font);
            (api.pango_font_description_free)(font);
            Some(Self { api, layout, map })
        }
    }

    pub(super) fn measure(&self, text: &str, width: u16, right: bool, markup: bool) -> u16 {
        unsafe {
            let api = &self.api;
            (api.pango_layout_set_width)(self.layout, i32::from(width) * 1024);
            (api.pango_layout_set_alignment)(self.layout, if right { 2 } else { 0 });
            if markup {
                (api.pango_layout_set_markup)(self.layout, text.as_ptr().cast(), text.len() as i32);
            } else {
                (api.pango_layout_set_attributes)(self.layout, ptr::null_mut());
                (api.pango_layout_set_text)(self.layout, text.as_ptr().cast(), text.len() as i32);
            }
            let mut h = 0;
            (api.pango_layout_get_pixel_size)(self.layout, ptr::null_mut(), &mut h);
            h.clamp(1, i32::from(u16::MAX)) as u16
        }
    }

    pub(super) fn render(&self, text: &str, width: u16, right: bool, markup: bool) -> macroquad::texture::Image {
        let height = self.measure(text, width, right, markup);
        let mut bytes = vec![0; usize::from(width) * usize::from(height) * 4];
        unsafe {
            let api = &self.api;
            // Cairo holds this buffer only between surface creation and destruction.
            let surface = (api.cairo_image_surface_create_for_data)(bytes.as_mut_ptr(), 0,
                i32::from(width), i32::from(height), i32::from(width)*4);
            let ctx = (api.cairo_create)(surface);
            let shade = 215./255.;
            (api.cairo_set_source_rgba)(ctx, shade, shade, shade, 1.);
            (api.pango_cairo_show_layout)(ctx, self.layout);
            (api.cairo_destroy)(ctx);
            (api.cairo_surface_flush)(surface);
            (api.cairo_surface_destroy)(surface);
        }
        straight_rgba(&mut bytes);
        macroquad::texture::Image { bytes, width, height }
    }
}

fn straight_rgba(bytes: &mut [u8]) {
    for pixel in bytes.chunks_exact_mut(4) {
        // Cairo ARGB32 is premultiplied BGRA in memory on Windows.
        let inv = (255 * 256_u32).checked_div(u32::from(pixel[3])).unwrap_or(0);
        for c in &mut pixel[..3] { *c = ((u32::from(*c) * inv / 256).min(255)) as u8; }
        pixel.swap(0, 2);
    }
}

#[cfg(test)]
mod tests {
    use super::straight_rgba;

    #[test]
    fn antialiased_tip_edges_use_the_native_integer_inverse_alpha() {
        let mut pixels = [0,0,0,0, 20,42,50,83, 17,88,201,255];
        straight_rgba(&mut pixels);
        assert_eq!(pixels, [0,0,0,0, 153,128,61,83, 201,88,17,255]);
    }
}
