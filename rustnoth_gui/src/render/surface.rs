//! Submit the composed battlefield to the window in one draw.
use macroquad::prelude::*;

pub(super) struct Surface {
    target: Option<RenderTarget>,
    copy: Material,
}

impl Surface {
    pub(super) fn new() -> Result<Self, macroquad::Error> {
        // The window is opaque. Copy the RGB produced by terrain and GUI alpha
        // blending without blending that finished color against the window again.
        let copy = load_material(ShaderSource::Glsl {
            vertex: r#"#version 100
                attribute vec3 position;
                attribute vec2 texcoord;
                uniform mat4 Model;
                uniform mat4 Projection;
                varying mediump vec2 uv;
                void main() {
                    gl_Position = Projection * Model * vec4(position, 1.0);
                    uv = texcoord;
                }"#,
            fragment: r#"#version 100
                precision mediump float;
                uniform sampler2D Texture;
                varying mediump vec2 uv;
                void main() { gl_FragColor = vec4(texture2D(Texture, uv).rgb, 1.0); }"#,
        }, MaterialParams::default())?;
        Ok(Self { target: None, copy })
    }

    pub(super) fn begin(&mut self) {
        let (width, height) = macroquad::miniquad::window::screen_size();
        let (width, height) = (width.max(1.0) as u32, height.max(1.0) as u32);
        if self.target.as_ref().is_none_or(|t| t.texture.width() as u32 != width || t.texture.height() as u32 != height) {
            let target = render_target(width, height);
            target.texture.set_filter(FilterMode::Nearest);
            self.target = Some(target);
        }
        let mut camera = crate::dialog::setup_camera(screen_width().max(1.), screen_height().max(1.));
        camera.render_target = self.target.clone();
        set_camera(&camera);
    }

    pub(super) fn present(&self) {
        set_default_camera();
        let target = self.target.as_ref().expect("battlefield surface was begun");
        gl_use_material(&self.copy);
        draw_texture_ex(&target.texture, 0., 0., WHITE, DrawTextureParams {
            dest_size: Some(vec2(screen_width(), screen_height())), ..Default::default()
        });
        gl_use_default_material();
    }
}
