use super::*;
use engine_lite::fixed_match::dialogue::DialogueLine;

const MAX_TEXT_WIDTH: f32 = 675.0;
const TITLE: Color = Color::from_rgba(186, 172, 125, 255);
const BODY: Color = Color::from_rgba(215, 215, 215, 255);
const FONT_SIZE: u16 = 17;
const LINE_HEIGHT: f32 = 21.0;

#[derive(Clone, Copy, Debug, PartialEq)]
enum PortraitLayout {
    Left,
    Right,
    Double,
}

impl PortraitLayout {
    fn for_message(line: &DialogueLine, image: &str) -> Self {
        if line.second_image.is_some() {
            Self::Double
        } else if !matches!(image, "" | "none")
            && line.portrait_right.unwrap_or_else(|| image.contains("~RIGHT()"))
        {
            Self::Right
        } else {
            Self::Left
        }
    }
}

// The spacer and foreground image use the size formulas from gui/window/wml_message.cfg.
// Portraits smaller than 300 x 300 keep their native pixels. Portraits in the 500 px
// format scale with the game map independently of the height of the text panel.
fn slot_width(view: Viewport, image: Vec2, layout: PortraitLayout) -> f32 {
    let width = if view.width > 37.5 { view.width - MAX_TEXT_WIDTH } else { view.width / 3.0 };
    let height_limit = if image.y > 0.0 {
        ((view.height - 30.0) * image.x / image.y).floor()
    } else {
        width
    };
    // The desktop window may be resized below Wesnoth's minimum supported resolution.
    let slots = if layout == PortraitLayout::Double { 2.0 } else { 1.0 };
    width.min(height_limit).clamp(250.0, 500.0)
        .min(((view.width - 120.0) / slots).max(0.0))
}

fn portrait_rect(view: Viewport, size: Vec2, layout: PortraitLayout, right: bool) -> Rect {
    let slot = slot_width(view, size, layout);
    let scale = if size.x < 300.0 && size.y < 300.0 {
        if slot < 250.0 { (slot / size.x).min(1.0) } else { 1.0 }
    } else {
        slot / 500.0
    };
    let image = (size * scale).floor();
    let x = if right {
        view.width - image.x
    } else if size.x <= 100.0 {
        ((slot - image.x) * 0.5).floor()
    } else {
        0.0
    };
    Rect::new(view.x + x, view.y + view.height - image.y, image.x, image.y)
}

fn text_column(view: Viewport, layout: PortraitLayout) -> (f32, f32) {
    // Spacer formulas run in the widget grid, before a foreground image sets its dimensions.
    let slot = slot_width(view, Vec2::ZERO, layout);
    let (left, right) = match layout {
        PortraitLayout::Left => (slot, 0.0),
        PortraitLayout::Right => {
            let inset = if view.width - 2.0 * slot > MAX_TEXT_WIDTH { slot } else { 10.0 };
            (inset, slot)
        }
        PortraitLayout::Double => (slot, slot),
    };
    (view.x + left + 5.0, ((view.width - left - right).min(MAX_TEXT_WIDTH) - 10.0).max(1.0))
}

fn portrait_flip(expression: &str, mirror: bool) -> (bool, bool) {
    let mut horizontal = mirror;
    let mut vertical = false;
    for modifier in expression.split('~').skip(1) {
        if let Some(args) = modifier.strip_prefix("FL(").and_then(|s| s.strip_suffix(')')) {
            horizontal ^= args.is_empty() || args.contains("horiz");
            vertical ^= args.contains("vert");
        }
    }
    (horizontal, vertical)
}

struct Portrait {
    texture: Texture2D,
    size: Vec2,
    flip: (bool, bool),
}

impl Portrait {
    fn load(image: &str, mirror: bool, speaker: Option<(&str, usize)>, assets: &AssetCatalog, art: &mut BoardArt) -> Option<Self> {
        let filename = image.split('~').next()?.trim();
        let (texture, size) = match filename {
            "" | "none" => return None,
            "unit_image" => {
                let (id, side) = speaker?;
                let texture = art.units.entry(id.to_owned()).or_default().entry(side).or_insert_with(|| {
                    load_texture(&assets.unit_image(id)?, art.team_colors.as_ref().map(|colors| (colors, side)))
                }).clone()?;
                // The Lua unit portrait accessor returns a team-colored 144x144 image for this fallback.
                (texture, vec2(144.0, 144.0))
            }
            explicit => {
                let texture = get_texture(&mut art.paths, assets.resolve_image(Path::new(explicit))?)?;
                let size = vec2(texture.width(), texture.height());
                (texture, size)
            }
        };
        Some(Self { texture, size, flip: portrait_flip(image, mirror) })
    }

    fn draw(&self, view: Viewport, layout: PortraitLayout, right: bool) {
        let rect = portrait_rect(view, self.size, layout, right);
        let window = Rect::new(view.x, 30.0, view.width, (view.y + view.height - 30.0).max(0.0));
        let Some(clip) = rect.intersect(window).filter(|r| r.w > 0.0 && r.h > 0.0) else { return };
        let (flip_x, flip_y) = self.flip;
        let dx = if flip_x { rect.x + rect.w - clip.x - clip.w } else { clip.x - rect.x };
        let dy = if flip_y { rect.y + rect.h - clip.y - clip.h } else { clip.y - rect.y };
        let sx = self.texture.width() / rect.w;
        let sy = self.texture.height() / rect.h;
        draw_texture_ex(&self.texture, clip.x, clip.y, WHITE, DrawTextureParams {
            source: Some(Rect::new(dx * sx, dy * sy, clip.w * sx, clip.h * sy)),
            dest_size: Some(vec2(clip.w, clip.h)), flip_x, flip_y, ..Default::default()
        });
    }
}

fn draw_message_panel(view: Viewport, top: f32, height: f32, art: &mut BoardArt, assets: &AssetCatalog) {
    let mut skin = |name| assets.resolve_image(Path::new(name)).and_then(|path| get_texture(&mut art.paths, path));
    if let Some(edge) = skin("dialogs/translucent65-border-top.png") {
        draw_texture_ex(&edge, view.x, top, WHITE, DrawTextureParams {
            dest_size: Some(vec2(view.width, edge.height())), ..Default::default()
        });
    }
    let body_height = (height - 5.0).max(0.0);
    if let Some(background) = skin("dialogs/translucent65-background.png") {
        draw_texture_ex(&background, view.x, top + 5.0, WHITE, DrawTextureParams {
            dest_size: Some(vec2(view.width, body_height)), ..Default::default()
        });
    } else {
        draw_rectangle(view.x, top + 5.0, view.width, body_height, Color::from_rgba(0, 0, 0, 166));
    }
}

/// Show an authored WML message over the game map. The caller advances pages and dismisses the line.
pub fn draw_dialogue(line: &DialogueLine, assets: &AssetCatalog, art: &mut BoardArt, page: usize) -> bool {
    let view = board_viewport();
    let kind = line.speaker_type.as_deref();
    let image = line.image.as_deref().or_else(|| {
        if line.second_image.as_deref().unwrap_or("").is_empty() {
            kind.map(|id| assets.dialogue_profile(id).unwrap_or("unit_image"))
        } else {
            None
        }
    }).unwrap_or("");
    let layout = PortraitLayout::for_message(line, image);
    let (left, width) = text_column(view, layout);
    let caption = line.caption.as_deref().or_else(|| kind.and_then(|id| assets.unit_name(id)).or(kind))
        .filter(|s| !s.is_empty()).map(plain_text);
    let title_height = if caption.is_some() { 37.0 } else { 10.0 };
    let lines = wrap(&plain_text(&line.text), width, |s|
        measure_text(s, art.dialogue_font.as_ref(), FONT_SIZE, 1.0).width);
    let bottom = view.y + view.height;
    let max_height = (bottom - 30.0).max(1.0);
    // Top/bottom panel borders are 10/13. The label and the overflow button each have 5px cell padding.
    let fixed_height = 10.0 + title_height + 10.0 + 13.0;
    let overflow = fixed_height + lines.len() as f32 * LINE_HEIGHT > max_height;
    let footer = if overflow { 31.0 } else { 0.0 };
    let per_page = ((max_height - fixed_height - footer) / LINE_HEIGHT).floor().max(1.0) as usize;
    let pages = lines.len().div_ceil(per_page).max(1);
    let page = page.min(pages - 1);
    let height = (fixed_height + lines.len().min(per_page) as f32 * LINE_HEIGHT + footer)
        .max(10.0 + 75.0 + 13.0).min(max_height);
    let top = bottom - height;
    draw_message_panel(view, top, height, art, assets);
    if let Some(caption) = caption {
        // GUI2's single-line title label ellipsizes a caption wider than its cell.
        let mut caption = caption;
        if measure_text(&caption, art.dialogue_font.as_ref(), 22, 1.0).width > width {
            while !caption.is_empty() && measure_text(&format!("{caption}…"), art.dialogue_font.as_ref(), 22, 1.0).width > width {
                caption.pop();
            }
            caption.push('…');
        }
        label(&art.dialogue_font, &caption, left, top + 36.0, 22, TITLE);
    }
    let mut baseline = top + 10.0 + title_height + 5.0 + FONT_SIZE as f32;
    for text in lines.iter().skip(page * per_page).take(per_page) {
        label(&art.dialogue_font, text, left, baseline, FONT_SIZE, BODY);
        baseline += LINE_HEIGHT;
    }
    if overflow {
        let label_width = measure_text("OK", art.dialogue_font.as_ref(), FONT_SIZE, 1.0).width;
        label(&art.dialogue_font, "OK", left + (width - label_width) / 2.0, bottom - 18.0, FONT_SIZE, TITLE);
    }
    // GUI2 composites the foreground after the text, so tall portraits extend up over the map.
    if let Some(portrait) = Portrait::load(image, line.mirror, kind.zip(line.speaker_side), assets, art) {
        portrait.draw(view, layout, layout == PortraitLayout::Right);
    }
    if let Some(second) = &line.second_image
        && let Some(portrait) = Portrait::load(second, line.second_mirror, None, assets, art)
    {
        portrait.draw(view, layout, true);
    }
    page + 1 == pages
}

// The desktop font draws plain text. Keep the contents of stock Pango spans and emphasis.
pub(super) fn plain_text(source: &str) -> String {
    let mut rest = source;
    let mut text = String::new();
    while let Some((before, after)) = rest.split_once('<') {
        text.push_str(before);
        let Some((tag, tail)) = after.split_once('>') else { text.push('<'); rest = after; break };
        let name = tag.trim_start_matches('/').split_whitespace().next().unwrap_or("");
        if matches!(name, "span" | "i" | "b" | "u" | "small" | "big" | "tt" | "s" | "sup" | "sub") {
            rest = tail;
        } else {
            text.push('<'); rest = after;
        }
    }
    text.push_str(rest);
    text.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

fn wrap(text: &str, width: f32, measure: impl Fn(&str) -> f32) -> Vec<String> {
    let mut result = Vec::new();
    for paragraph in text.split('\n') {
        let mut line = String::new();
        for word in paragraph.split_whitespace() {
            let candidate = if line.is_empty() { word.to_owned() } else { format!("{line} {word}") };
            if measure(&candidate) <= width { line = candidate; continue; }
            if !line.is_empty() { result.push(std::mem::take(&mut line)); }
            for ch in word.chars() {
                line.push(ch);
                if line.len() > ch.len_utf8() && measure(&line) > width {
                    line.pop();
                    result.push(std::mem::take(&mut line));
                    line.push(ch);
                }
            }
        }
        result.push(line);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_columns_and_portraits_use_game_map_coordinates() {
        let view = Viewport { x: 7.0, y: 27.0, width: 1018.0, height: 793.0 };
        assert_eq!(text_column(view, PortraitLayout::Left), (355.0, 665.0));
        assert_eq!(text_column(view, PortraitLayout::Right), (22.0, 655.0));
        assert_eq!(text_column(view, PortraitLayout::Double), (355.0, 322.0));
        let size = vec2(500.0, 500.0);
        assert_eq!(portrait_rect(view, size, PortraitLayout::Left, false), Rect::new(7.0, 477.0, 343.0, 343.0));
        assert_eq!(portrait_rect(view, size, PortraitLayout::Double, true), Rect::new(682.0, 477.0, 343.0, 343.0));
        assert_eq!(portrait_rect(view, vec2(72.0, 72.0), PortraitLayout::Left, false),
            Rect::new(142.0, 748.0, 72.0, 72.0));
    }

    #[test]
    fn profile_direction_can_be_overridden_and_double_dialogues_anchor_both_speakers() {
        let mut line = DialogueLine { text: "Stay.".into(), caption: None, image: None, speaker_type: None,
            speaker_side: None, portrait_right: None, mirror: false, second_image: None, second_mirror: false };
        let image = "portraits/kapoue.webp~FL()~RIGHT()";
        assert_eq!(PortraitLayout::for_message(&line, image), PortraitLayout::Right);
        assert_eq!(portrait_flip(image, false), (true, false));
        assert_eq!(portrait_flip(image, true), (false, false));
        line.portrait_right = Some(false);
        assert_eq!(PortraitLayout::for_message(&line, image), PortraitLayout::Left);
        line.portrait_right = Some(true);
        assert_eq!(PortraitLayout::for_message(&line, "none"), PortraitLayout::Left);
        line.second_image = Some("portraits/gruu.webp".into());
        assert_eq!(PortraitLayout::for_message(&line, image), PortraitLayout::Double);
    }

    #[test]
    fn stock_markup_and_long_lines_keep_all_the_dialogue_text() {
        let text = plain_text("<span color='#BAAC7D'>We ride <i>east</i> &amp; north.</span>");
        assert_eq!(text, "We ride east & north.");
        assert_eq!(wrap("a bcde\n\nfront", 3.0, |s| s.len() as f32), ["a", "bcd", "e", "", "fro", "nt"]);
    }
}
