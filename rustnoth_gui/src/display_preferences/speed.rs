use super::config_directory;
use std::{fs, io, path::Path};

pub(crate) const FACTORS: [f32; 12] = [0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0, 3.0, 4.0, 8.0, 16.0];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Speed {
    pub scroll: u8,
    pub accelerated: bool,
    pub factor: u8,
}

impl Default for Speed {
    fn default() -> Self {
        Self { scroll: 50, accelerated: false, factor: 7 }
    }
}

impl Speed {
    pub fn load() -> io::Result<Self> {
        Self::read(&config_directory()?)
    }

    fn read(dir: &Path) -> io::Result<Self> {
        let data = match fs::read_to_string(dir.join("game-speed")) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            data => data?,
        };
        let invalid = || io::Error::new(io::ErrorKind::InvalidData, "Invalid Rustnoth game speed settings");
        let mut values = data.split_whitespace();
        let scroll = values.next().and_then(|s| s.parse().ok()).filter(|s| (1..=100).contains(s)).ok_or_else(invalid)?;
        let accelerated = values.next().and_then(|s| s.parse().ok()).ok_or_else(invalid)?;
        let factor = values.next().and_then(|s| s.parse().ok()).filter(|s| usize::from(*s) < FACTORS.len()).ok_or_else(invalid)?;
        if values.next().is_some() { return Err(invalid()); }
        Ok(Self { scroll, accelerated, factor })
    }

    pub fn save(self) -> io::Result<()> {
        self.write(&config_directory()?)
    }

    fn write(self, dir: &Path) -> io::Result<()> {
        fs::create_dir_all(dir)?;
        let pending = dir.join(format!("game-speed.{}.tmp", std::process::id()));
        fs::write(&pending, format!("{} {} {}\n", self.scroll, self.accelerated, self.factor))?;
        fs::rename(pending, dir.join("game-speed"))
    }

    pub fn pan_distance(self, seconds: f32) -> f32 {
        36.0 * f32::from(self.scroll) * seconds
    }

    pub fn animation_rate(self, shift: bool) -> f32 {
        if self.accelerated ^ shift { FACTORS[usize::from(self.factor)] } else { 1.0 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restored_speeds_control_panning_and_shift_inverts_acceleration() {
        let unique = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("rustnoth-speed-{}-{unique}", std::process::id()));
        assert_eq!(Speed::read(&dir).unwrap(), Speed::default());
        assert_eq!(Speed::default().animation_rate(true), 2.0);
        let fast = Speed { scroll: 100, accelerated: true, factor: 9 };
        fast.write(&dir).unwrap();
        let read = Speed::read(&dir).unwrap();
        assert_eq!(read, fast);
        assert_eq!(read.pan_distance(0.25), 900.0);
        assert_eq!((read.animation_rate(false), read.animation_rate(true)), (4.0, 1.0));
        fs::remove_file(dir.join("game-speed")).unwrap();
        fs::remove_dir(dir).unwrap();
    }
}
