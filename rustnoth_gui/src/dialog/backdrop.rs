//! Freeze the composed battlefield on the GPU for a modal's lifetime.
use macroquad::{
    miniquad::{TextureAccess, TextureFormat, TextureParams, TextureSource, window},
    prelude::*,
};

pub(crate) struct Backdrop(Texture2D);

impl Backdrop {
    pub(crate) fn capture(filter: FilterMode) -> Self {
        // Flush the composed frame before copying from the window's back buffer.
        let raw = unsafe {
            let mut gl = get_internal_gl();
            gl.flush();
            gl.quad_context.new_texture(
                TextureAccess::Static,
                TextureSource::Empty,
                capture_params(window::screen_size()),
            )
        };
        let texture = Texture2D::from_miniquad_texture(raw);
        texture.grab_screen();
        texture.set_filter(filter);
        Self(texture)
    }

    pub(crate) fn texture(&self) -> &Texture2D {
        &self.0
    }
}

impl Drop for Backdrop {
    fn drop(&mut self) {
        let raw = self.0.raw_miniquad_id();
        // Macroquad may still have a draw queued against this unmanaged handle.
        unsafe {
            let mut gl = get_internal_gl();
            gl.flush();
            gl.quad_context.delete_texture(raw);
        }
    }
}

fn capture_params((width, height): (f32, f32)) -> TextureParams {
    TextureParams {
        width: width as u32,
        height: height as u32,
        // The composed color is opaque. The window's alpha channel can still
        // contain intermediate alpha values from drawing translucent layers.
        format: TextureFormat::RGB8,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modal_snapshot_keeps_physical_pixels_and_discards_composited_alpha() {
        let params = capture_params((2400.0, 1640.0));
        assert_eq!((params.width, params.height), (2400, 1640));
        assert_eq!(params.format, TextureFormat::RGB8);
    }
}
