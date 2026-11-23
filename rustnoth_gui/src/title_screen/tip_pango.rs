//! Windows installations supply the Pango, Cairo and FreeType stack used by Wesnoth.
mod dialogue;
use std::{ffi::{CString, c_char, c_void}, os::windows::ffi::OsStrExt, path::Path, ptr, rc::Rc};

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
        fn pango_parse_markup(*const c_char, i32, u32, *mut Handle, *mut *mut c_char, *mut u32, *mut Handle) -> i32;
        fn pango_layout_set_height(Handle, i32);
        fn pango_layout_set_ellipsize(Handle, i32);
        fn pango_layout_get_iter(Handle) -> Handle;
        fn pango_layout_iter_get_line_yrange(Handle, *mut i32, *mut i32);
        fn pango_layout_iter_next_line(Handle) -> i32;
        fn pango_layout_iter_free(Handle);
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
        fn pango_cairo_layout_path(Handle, Handle);
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
        fn cairo_translate(Handle, f64, f64);
        fn cairo_set_source_rgba(Handle, f64, f64, f64, f64);
        fn cairo_set_line_join(Handle, i32);
        fn cairo_set_line_width(Handle, f64);
        fn cairo_stroke(Handle);
        fn cairo_destroy(Handle);
    }
}

pub(crate) struct Text {
    api: Rc<Api>,
    layout: Handle,
    map: Handle,
    font: Handle,
}

impl Drop for Text {
    fn drop(&mut self) {
        unsafe {
            (self.api.pango_font_description_free)(self.font);
            (self.api.g_object_unref)(self.layout);
            (self.api.g_object_unref)(self.map);
        }
    }
}

impl Text {
    pub(crate) fn load(assets: &Path, family: &str) -> Option<Self> {
        // Asset paths supplied on the command line are data only. Resolve DLLs through Steam.
        // Keep Pango loaded between renderers. GObject can retain its process-wide
        // type registrations after an individual Pango DLL has been unloaded.
        thread_local! {
            static INSTALLED_API: Option<Rc<Api>> = crate::desktop::steam_game_roots()
                .iter().find_map(|dir| Api::load(dir)).map(Rc::new);
        }
        let api = INSTALLED_API.with(Clone::clone)?;
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
            Some(Self { api, layout, map, font })
        }
    }

    fn prepare(&self, text: &str, size: u16, width: Option<u16>, alignment: i32, markup: bool) -> (u16, u16) {
        unsafe {
            let api = &self.api;
            (api.pango_font_description_set_size)(self.font, i32::from(size) * 1024);
            (api.pango_layout_set_font_description)(self.layout, self.font);
            (api.pango_layout_set_width)(self.layout, width.map_or(-1, |w| i32::from(w) * 1024));
            (api.pango_layout_set_alignment)(self.layout, alignment);
            if markup {
                (api.pango_layout_set_markup)(self.layout, text.as_ptr().cast(), text.len() as i32);
            } else {
                (api.pango_layout_set_attributes)(self.layout, ptr::null_mut());
                (api.pango_layout_set_text)(self.layout, text.as_ptr().cast(), text.len() as i32);
            }
            let (mut w, mut h) = (0, 0);
            (api.pango_layout_get_pixel_size)(self.layout, &mut w, &mut h);
            let dimension = |n: i32| n.clamp(1, i32::from(u16::MAX)) as u16;
            (dimension(w), dimension(h))
        }
    }

    pub(super) fn measure(&self, text: &str, width: u16, right: bool, markup: bool) -> u16 {
        self.prepare(text, 17, Some(width), if right { 2 } else { 0 }, markup).1
    }

    pub(super) fn render(&self, text: &str, width: u16, right: bool, markup: bool) -> macroquad::texture::Image {
        let height = self.measure(text, width, right, markup);
        self.rasterize(width, height, [215; 3])
    }

    pub(crate) fn text_width(&self, text: &str, size: u16) -> u16 {
        self.prepare(text, size, None, 0, false).0
    }

    pub(crate) fn button(&self, text: &str, size: u16, color: [u8; 3]) -> macroquad::texture::Image {
        let (width, height) = self.prepare(text, size, None, 0, false);
        self.rasterize(width, height, color)
    }

    pub(crate) fn hex_report(&self, text: &str) -> macroquad::texture::Image {
        let (width, height) = self.prepare(text, 13, None, 1, false);
        self.rasterize(width, height, [221; 3])
    }

    pub(crate) fn battlefield_report(&self, text: &str, size: u16) -> macroquad::texture::Image {
        let (width, height) = self.prepare(text, size, None, 0, false);
        self.rasterize_label(width, height, [221; 3], 0, true)
    }

    pub(crate) fn paragraph(&self, text: &str, width: u16, alignment: i32) -> macroquad::texture::Image {
        let (_, height) = self.prepare(text, 17, Some(width), alignment, true);
        self.rasterize(width, height, [215; 3])
    }

    pub(crate) fn fitted_label(&self, text: &str, size: u16, limit: u16, markup: bool, color: [u8; 3]) -> Option<macroquad::texture::Image> {
        let (width, height) = self.prepare(text, size, Some(limit), 0, markup);
        // Keep long objectives scrollable without requiring an oversized GPU texture.
        (height <= 2048).then(|| self.rasterize(width, height, color))
    }

    pub(crate) fn tutorial_hint(&self, markup: &str, limit: u16) -> macroquad::texture::Image {
        let (width, height) = self.prepare(markup, 18, Some(limit), 0, true);
        self.rasterize(width, height, [255; 3])
    }

    pub(crate) fn tutorial_print(&self, text: &str, width: u16) -> macroquad::texture::Image {
        let (width, height) = self.prepare(text, 18, Some(width), 0, true);
        self.rasterize_label(width, height, [255; 3], 0, true)
    }

    fn rasterize(&self, width: u16, height: u16, color: [u8; 3]) -> macroquad::texture::Image {
        self.rasterize_label(width, height, color, 0, false)
    }

    fn rasterize_label(&self, width: u16, height: u16, color: [u8; 3], top: i32, outline: bool) -> macroquad::texture::Image {
        let mut bytes = vec![0; usize::from(width) * usize::from(height) * 4];
        unsafe {
            let api = &self.api;
            // Cairo holds this buffer only between surface creation and destruction.
            let surface = (api.cairo_image_surface_create_for_data)(bytes.as_mut_ptr(), 0,
                i32::from(width), i32::from(height), i32::from(width)*4);
            let ctx = (api.cairo_create)(surface);
            (api.cairo_translate)(ctx, 0., -f64::from(top));
            if outline {
                (api.pango_cairo_layout_path)(ctx, self.layout);
                (api.cairo_set_source_rgba)(ctx, 0., 0., 0., 1.);
                (api.cairo_set_line_join)(ctx, 1); // CAIRO_LINE_JOIN_ROUND
                (api.cairo_set_line_width)(ctx, 3.);
                (api.cairo_stroke)(ctx);
            }
            let [r, g, b] = color.map(|c| f64::from(c) / 255.);
            (api.cairo_set_source_rgba)(ctx, r, g, b, 1.);
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
    use super::{Text, straight_rgba};
    use std::rc::Rc;

    #[test]
    #[ignore = "requires the installed Wesnoth Windows libraries and fonts"]
    fn pango_libraries_stay_loaded_between_renderers() {
        let root = crate::desktop::steam_game_roots().into_iter()
            .find(|root| root.join("fonts/fonts.conf").is_file())
            .expect("installed Wesnoth fonts");
        let first = Text::load(&root, "Lato").expect("first native text renderer");
        let api = Rc::downgrade(&first.api);
        let width = first.text_width("Battlefield", 15);
        drop(first);

        let pinned = api.strong_count();
        assert!(pinned > 0, "Pango was unloaded with the last text renderer");
        let second = Text::load(&root, "Lato").expect("second native text renderer");
        assert!(Rc::ptr_eq(&api.upgrade().unwrap(), &second.api));
        assert_eq!(second.text_width("Battlefield", 15), width);
    }

    #[test]
    fn antialiased_tip_edges_use_the_native_integer_inverse_alpha() {
        let mut pixels = [0,0,0,0, 20,42,50,83, 17,88,201,255];
        straight_rgba(&mut pixels);
        assert_eq!(pixels, [0,0,0,0, 153,128,61,83, 201,88,17,255]);
    }
}
