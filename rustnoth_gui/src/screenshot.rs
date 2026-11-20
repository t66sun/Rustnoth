use image::{ImageEncoder, codecs::png::PngEncoder};
use macroquad::prelude::*;
use std::{error::Error, fs::{self, File}, io, path::{Path, PathBuf}};

fn save(dir: &Path, frame: Image) -> Result<PathBuf, Box<dyn Error>> {
    fs::create_dir_all(dir)?;
    let (path, mut file) = (1u64..).find_map(|index| {
        let path = dir.join(format!("Screenshot_{index:03}.png"));
        match File::create_new(&path) {
            Ok(file) => Some(Ok((path, file))),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => None,
            Err(error) => Some(Err(error)),
        }
    }).expect("screenshot index space exhausted")?;
    let mut pixels = Vec::with_capacity(usize::from(frame.width) * usize::from(frame.height) * 3);
    for row in frame.bytes.chunks_exact(usize::from(frame.width) * 4).rev() {
        for pixel in row.chunks_exact(4) {
            pixels.extend_from_slice(&pixel[..3]);
        }
    }
    let result = PngEncoder::new(&mut file).write_image(&pixels, frame.width.into(), frame.height.into(), image::ColorType::Rgb8);
    if let Err(error) = result {
        drop(file);
        fs::remove_file(&path)?;
        return Err(error.into());
    }
    Ok(path)
}

pub(super) async fn listen() {
    let mut notice: Option<(String, f64)> = None;
    loop {
        if is_key_pressed(KeyCode::F12) {
            let result = crate::display_preferences::config_directory().map_err(Box::<dyn Error>::from)
                .and_then(|dir| save(&dir.join("screenshots"), get_screen_data()));
            let message = match result {
                Ok(path) => format!("Saved {}", path.display()),
                Err(error) => format!("Could not save screenshot: {error}"),
            };
            notice = Some((message, get_time() + 2.0));
        }
        if let Some((message, until)) = &notice {
            if get_time() < *until {
                set_default_camera();
                let width = measure_text(message, None, 16, 1.).width;
                draw_rectangle(8., 8., width + 16., 26., Color::from_rgba(0, 0, 0, 210));
                draw_text(message, 16., 26., 16., WHITE);
            } else {
                notice = None;
            }
        }
        next_frame().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn capture_preserves_existing_files_and_writes_upright_opaque_rgb() {
        let dir = std::env::temp_dir().join(format!("rustnoth-screenshot-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("Screenshot_001.png"), b"old").unwrap();
        let frame = Image { width: 1, height: 2, bytes: vec![0, 1, 2, 0, 250, 251, 252, 128] };
        let path = save(&dir, frame).unwrap();
        assert_eq!(path.file_name().unwrap(), "Screenshot_002.png");
        assert_eq!(image::open(path).unwrap().into_rgb8().as_raw(), &[250,251,252,0,1,2]);
        assert_eq!(fs::read(dir.join("Screenshot_001.png")).unwrap(), b"old");
        fs::remove_dir_all(dir).unwrap();
    }
}
