//! Original contributor credits in the order used by end_credits.
use super::{Art, image, panel};
use crate::dialog;
use engine_lite::wml::{self, Node};
use macroquad::prelude::*;
use std::{
    error::Error,
    path::{Component, Path},
};

struct Line {
    text: String,
    size: u16,
    y: f32,
}
#[derive(Default)]
struct Roll {
    lines: Vec<Line>,
    height: f32,
    background: Option<String>,
}

impl Roll {
    fn line(&mut self, text: &str, size: u16) {
        self.height += f32::from(size) + 6.0;
        self.lines.push(Line {
            text: text.to_owned(),
            size,
            y: self.height,
        });
    }
    fn group(&mut self, group: &Node, campaign: bool) {
        let mut sections: Vec<_> = group
            .children
            .iter()
            .filter(|(tag, sec)| tag == "about" && sec.children.iter().any(|(t, _)| t == "entry"))
            .map(|(_, s)| s)
            .collect();
        if sections.is_empty() {
            return;
        }
        if matches!(group.attribute("sort"), Some("yes" | "true")) {
            sections.sort_by_key(|s| s.attribute("title").unwrap_or("").to_lowercase());
        }
        self.height += 22.0;
        if campaign && let Some(header) = group.attribute("name") {
            self.line(header, 32);
        }
        for section in sections {
            if !campaign && self.background.is_none() {
                self.background = section
                    .attribute("images")
                    .and_then(|s| s.split(',').next())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned);
            }
            self.height += 17.0;
            if let Some(title) = section.attribute("title") {
                self.line(title, 27);
            }
            for (_, entry) in section.children.iter().filter(|(tag, _)| tag == "entry") {
                if let Some(name) = entry.attribute("name") {
                    self.line(name, 17);
                }
            }
        }
    }
    fn read(core: &Node, catalog: Option<&Node>) -> Self {
        let mut roll = Self::default();
        let primary = catalog
            .filter(|root| root.children.iter().any(|(t, _)| t == "credits_group"))
            .unwrap_or(core);
        for (_, group) in primary
            .children
            .iter()
            .filter(|(t, _)| t == "credits_group")
        {
            roll.group(group, false);
        }
        roll.group(primary, false);
        for (_, group) in catalog
            .unwrap_or(primary)
            .children
            .iter()
            .filter(|(t, _)| t == "campaign")
        {
            roll.group(group, true);
        }
        roll
    }
}

pub(super) async fn show(
    root: &Path,
    core: &[u8],
    campaign: Option<&[u8]>,
    art: &Art,
) -> Result<(), Box<dyn Error>> {
    let core = wml::parse(core)?;
    let extra = campaign.map(wml::parse).transpose()?;
    let roll = Roll::read(&core, extra.as_ref());
    if roll.lines.is_empty() {
        return Err("No contributor credits found in the supplied game data".into());
    }
    let background = roll
        .background
        .as_deref()
        .map(Path::new)
        .filter(|f| f.components().all(|c| matches!(c, Component::Normal(_))))
        .and_then(|f| {
            [
                root.join("images"),
                root.join("data/core/images"),
                root.to_owned(),
            ]
            .into_iter()
            .find_map(|p| crate::render::load_texture(&p.join(f), None))
        });
    let mut offset = 0.0f32;
    let mut speed = 100.0f32;
    loop {
        let scale = (screen_width() / 520.0)
            .min(screen_height() / 340.0)
            .clamp(0.01, 1.0);
        let (w, h) = (screen_width() / scale, screen_height() / scale);
        set_camera(&dialog::setup_camera(w, h));
        image(
            Rect::new(0.0, 0.0, w, h),
            background.as_ref().unwrap_or(&art.map),
        );
        let bounds = Rect::new(30.0, 30.0, w - 60.0, h - 113.0);
        panel(bounds);
        let first = roll.lines.partition_point(|l| l.y < offset);
        for line in &roll.lines[first..] {
            let y = bounds.y + line.y - offset;
            if y > bounds.bottom() - 8.0 {
                break;
            }
            if y - f32::from(line.size) < bounds.y + 4.0 {
                continue;
            }
            let text = if line.size > 20 {
                art.locale.game(&line.text)
            } else {
                &line.text
            };
            let m = measure_text(text, art.font.as_ref(), line.size, 1.0);
            dialog::text(
                art.font.as_ref(),
                text,
                (w - m.width) * 0.5,
                y,
                line.size,
                WHITE,
            );
        }
        let hint = art.locale.ui("↑ ↓ to adjust speed");
        let width = measure_text(hint, art.font.as_ref(), 14, 1.0).width;
        dialog::text(
            art.font.as_ref(),
            hint,
            bounds.right() - width - 6.0,
            bounds.bottom() - 10.0,
            14,
            WHITE,
        );
        let close = Rect::new((w - 105.0) / 2.0, h - 53.0, 105.0, 33.0);
        let hover = close.contains(Vec2::from(mouse_position()) / scale);
        art.button(
            close,
            art.locale.ui("Close"),
            hover,
            hover && is_mouse_button_down(MouseButton::Left),
        );
        set_default_camera();
        if is_key_pressed(KeyCode::Escape)
            || is_key_pressed(KeyCode::Enter)
            || hover && is_mouse_button_pressed(MouseButton::Left)
        {
            return Ok(());
        }
        if is_key_pressed(KeyCode::Up) {
            speed = (speed * 2.0).min(400.0);
        }
        if is_key_pressed(KeyCode::Down) {
            speed = (speed / 2.0).max(50.0);
        }
        offset = (offset + get_frame_time() * speed - mouse_wheel().1 * 35.0)
            .clamp(0.0, (roll.height - bounds.h + 14.0).max(0.0));
        next_frame().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn general_credits_appear_once_before_campaigns_and_preserve_separators() {
        let general = "[credits_group]\n[about]\ntitle=Programming\n[entry]\nname=Founder\n[/entry]\n[entry]\nname=•\n[/entry]\n[/about]\n[/credits_group]";
        let extra = format!(
            "{general}\n[campaign]\nname=River\n[about]\ntitle=Design\n[entry]\nname=Cartographer\n[/entry]\n[/about]\n[/campaign]"
        );
        let core = wml::parse(general).unwrap();
        let full = wml::parse(extra).unwrap();
        let roll = Roll::read(&core, Some(&full));
        assert_eq!(
            roll.lines
                .iter()
                .map(|l| l.text.as_str())
                .collect::<Vec<_>>(),
            [
                "Programming",
                "Founder",
                "•",
                "River",
                "Design",
                "Cartographer"
            ]
        );
    }
}
