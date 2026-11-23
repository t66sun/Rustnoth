//! The default theme has separate image and schedule reports, each with its own tooltip.
use super::*;
use crate::title_screen::locale::{self, Locale};
use engine_lite::fixed_match::TimeReport;
use std::cell::OnceCell;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Hover { Period, Schedule }

pub(super) struct Art {
    root: PathBuf,
    images: HashMap<String, Texture>,
    locale: OnceCell<Option<Locale>>,
    hover: Option<(Hover, f64)>,
    #[cfg(windows)] font: OnceCell<Option<crate::title_screen::tip_pango::Text>>,
    #[cfg(windows)] number: Option<(usize, usize, Texture2D)>,
    #[cfg(windows)] tooltip: Option<(String, Texture2D)>,
}

impl Art {
    pub(super) fn new(root: &Path) -> Self {
        Self { root: root.into(), images: HashMap::new(), locale: OnceCell::new(), hover: None,
            #[cfg(windows)] font: OnceCell::new(), #[cfg(windows)] number: None,
            #[cfg(windows)] tooltip: None }
    }

    fn image(&mut self, name: &str, rect: Rect, assets: &AssetCatalog) {
        if name.is_empty() { return; }
        if !self.images.contains_key(name) {
            let image = assets.resolve_image(Path::new(name)).and_then(|p| load_texture(&p, None));
            self.images.insert(name.into(), image);
        }
        paint(&self.images[name], rect);
    }

    pub(super) fn draw(&mut self, report: &TimeReport<'_>, assets: &AssetCatalog,
        image: Rect, stats: Rect, scale: f32, fallback: &Option<Font>, interactive: bool)
    {
        let period = &report.schedule[report.current];
        self.image(&period.image, image, assets);
        match report.lawful.cmp(&period.lawful_bonus) {
            std::cmp::Ordering::Greater => self.image("misc/tod-bright.png", image, assets),
            std::cmp::Ordering::Less => self.image("misc/tod-dark.png", image, assets),
            _ => {}
        }
        let current = report.current + 1;
        let total = report.schedule.len();
        #[cfg(windows)]
        let rasterized = if let Some(font) = self.font.get_or_init(|| crate::title_screen::tip_pango::Text::load(&self.root, "Lato")) {
            if !self.number.as_ref().is_some_and(|(n, len, _)| (*n, *len) == (current, total)) {
                let label = font.battlefield_report(&format!("{current}/{total}"), 15);
                let texture = Texture2D::from_image(&label);
                texture.set_filter(FilterMode::Nearest);
                self.number = Some((current, total, texture));
            }
            let texture = &self.number.as_ref().unwrap().2;
            draw_texture_ex(texture, stats.x, stats.y, WHITE, DrawTextureParams {
                dest_size: Some(texture.size() * scale), ..Default::default()
            });
            true
        } else { false };
        #[cfg(not(windows))]
        let rasterized = false;
        if !rasterized { text_in(fallback, &format!("{current}/{total}"), stats, 15, TEXT, false); }

        let pt = Vec2::from(mouse_position());
        let hover = if !interactive { None } else if image.contains(pt) { Some(Hover::Period) }
            else if stats.contains(pt) { Some(Hover::Schedule) } else { None };
        let Some(hover) = hover else { self.hover = None; return };
        let time = get_time();
        if self.hover.map(|last| last.0) != Some(hover) { self.hover = Some((hover, time)); }
        if time - self.hover.unwrap().1 < 0.4 { return; }

        self.locale.get_or_init(|| {
            let selected = locale::saved(&crate::display_preferences::config_directory().ok()?).ok()?;
            Locale::load(&self.root, selected).ok()
        });
        let content = tooltip(hover, report, &self.root, self.locale.get_mut().and_then(Option::as_mut));
        #[cfg(windows)]
        if let Some(font) = self.font.get().and_then(Option::as_ref) {
            if !self.tooltip.as_ref().is_some_and(|(text, _)| text == &content)
                && let Some(pixels) = font.fitted_label(&content, 15, screen_width().min(420.).max(1.) as u16, true, [221; 3]) {
                let texture = Texture2D::from_image(&pixels);
                texture.set_filter(FilterMode::Nearest);
                self.tooltip = Some((content.clone(), texture));
            }
            if let Some((text, texture)) = &self.tooltip && text == &content {
                let at = tooltip_background(image.x, pt.y, texture.size());
                draw_texture(texture, at.x, at.y, WHITE);
                return;
            }
        }
        let plain = super::super::dialogue::plain_text(&content);
        let size = vec2(plain.lines().map(|line| measure_text(line, fallback.as_ref(), 15, 1.).width).fold(0., f32::max),
            19. * plain.lines().count() as f32);
        let at = tooltip_background(image.x, pt.y, size);
        for (i, line) in plain.lines().enumerate() {
            crate::dialog::text(fallback.as_ref(), line, at.x, at.y + i as f32 * 19. + 15., 15, TEXT);
        }
    }
}

fn tooltip_background(right: f32, top: f32, size: Vec2) -> Vec2 {
    let at = vec2((right - size.x - 12.).max(4.), top.min(screen_height() - size.y - 5.).max(4.));
    draw_rectangle(at.x - 4., at.y - 3., size.x + 8., size.y + 6., Color::from_rgba(0, 0, 0, 240));
    at
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn tooltip(hover: Hover, report: &TimeReport<'_>, root: &Path, mut locale: Option<&mut Locale>) -> String {
    let names: Vec<_> = report.schedule.iter().map(|period| {
        let name = locale.as_deref_mut().map(|l| l.value(root, &period.name))
            .unwrap_or_else(|| period.name.text.clone());
        escape(&name)
    }).collect();
    let tr = |s: &str| escape(locale.as_deref().map_or(s, |l| l.game(s)));
    if hover == Hover::Schedule {
        let mut text = tr("Time of day schedule:");
        for (index, name) in names.iter().enumerate() {
            text.push('\n');
            if index == report.current { text.push_str(&format!("<big><b>{name}</b></big>")); }
            else { text.push_str(name); }
        }
        return text;
    }
    let mut text = format!("{} <b>{}</b>", tr("Time of day:"), names[report.current]);
    let lawful = i64::from(report.lawful);
    for (key, bonus) in [("Lawful units: ", lawful), ("Neutral units: ", 0),
        ("Chaotic units: ", -lawful), ("Liminal units: ", report.liminal)] {
        let color = match bonus.cmp(&0) {
            std::cmp::Ordering::Greater => "#00ff00", std::cmp::Ordering::Less => "#ff0000",
            std::cmp::Ordering::Equal => "#ffffff",
        };
        let sign = if bonus < 0 { '−' } else { '+' };
        text.push_str(&format!("\n{}<span foreground='{color}'>{sign}{}%</span>", tr(key), bonus.unsigned_abs()));
    }
    text
}
