use super::*;
use engine_lite::fixed_match::dialogue::DialogueLine;

/// Draw the WML message panel using the original translucent skin. Returns true on the final page.
pub fn draw_dialogue(line: &DialogueLine, assets: &AssetCatalog, art: &mut BoardArt, page: usize) -> bool {
    let width = screen_width();
    let height = (screen_height() * 0.42).clamp(145.0, 300.0).min(screen_height());
    let top = screen_height() - height;
    let texture = |art: &mut BoardArt, name: &str| get_texture(&mut art.paths, assets.resolve_image(Path::new(name))?);
    let panel = texture(art, "dialogs/translucent65-background.png");
    if let Some(panel) = panel {
        draw_texture_ex(&panel, 0.0, top, WHITE, DrawTextureParams {
            dest_size: Some(vec2(width, height)), ..Default::default()
        });
    } else { draw_rectangle(0.0, top, width, height, PANEL); }
    if let Some(edge) = texture(art, "dialogs/translucent65-border-top.png") {
        draw_texture_ex(&edge, 0.0, top, WHITE, DrawTextureParams {
            dest_size: Some(vec2(width, edge.height())), ..Default::default()
        });
    }

    let kind = line.speaker_type.as_deref();
    let portrait = match line.image.as_deref() {
        Some("none") => None,
        Some("unit_image") => kind.and_then(|id| assets.unit_image(id)),
        Some(image) => assets.resolve_image(Path::new(image.split('~').next().unwrap_or(image))),
        None => kind.and_then(|id| assets.dialogue_portrait(id).or_else(|| assets.unit_image(id))),
    }.and_then(|path| get_texture(&mut art.paths, path));
    let mut left = 18.0;
    let mut right = width - 18.0;
    if let Some(portrait) = portrait.filter(|_| width > 360.0) {
        let size = vec2(portrait.width(), portrait.height());
        let scale = ((height - 20.0) / size.y).min((width * 0.27) / size.x);
        let size = size * scale;
        let x = if line.portrait_right { right -= size.x + 14.0; right + 14.0 }
            else { left += size.x + 14.0; 18.0 };
        draw_texture_ex(&portrait, x, top + height - size.y, WHITE, DrawTextureParams {
            dest_size: Some(size), ..Default::default()
        });
    }
    let caption = line.caption.as_deref().or_else(|| kind.and_then(|id| assets.unit_name(id))).or(kind);
    let mut baseline = top + 33.0;
    if let Some(caption) = caption {
        label(&art.font, &plain_text(caption), left, baseline, 23, SELECTED);
        baseline += 30.0;
    }
    let font_size = 19;
    let lines = wrap(&plain_text(&line.text), (right - left).max(8.0), |s|
        measure_text(s, art.font.as_ref(), font_size, 1.0).width);
    let per_page = (((screen_height() - 36.0 - baseline) / 25.0).floor() as usize + 1).max(1);
    let pages = lines.len().div_ceil(per_page).max(1);
    let page = page.min(pages - 1);
    for text in lines.iter().skip(page * per_page).take(per_page) {
        label(&art.font, text, left, baseline, font_size, TEXT);
        baseline += 25.0;
    }
    label(&art.font, "Click or press Enter to continue", left, screen_height() - 10.0, 15, MUTED);
    page + 1 == pages
}

// The desktop font draws plain text. Keep the contents of stock Pango spans and emphasis.
fn plain_text(source: &str) -> String {
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
    fn stock_markup_and_long_lines_keep_all_the_dialogue_text() {
        let text = plain_text("<span color='#BAAC7D'>We ride <i>east</i> &amp; north.</span>");
        assert_eq!(text, "We ride east & north.");
        assert_eq!(wrap("a bcde\n\nfront", 3.0, |s| s.len() as f32), ["a", "bcd", "e", "", "fro", "nt"]);
    }
}
