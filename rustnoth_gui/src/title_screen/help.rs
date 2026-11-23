//! Native help browser opened by the title screen's Help button or F1.
mod book;

use super::{Art, centered, dialog, image, load_texture, preferences::tiled};
use book::{Book, Span, Topic};
use macroquad::prelude::*;
use std::{
    collections::{HashMap, HashSet},
    error::Error,
    path::{Component, Path},
};

const GOLD: Color = Color::from_rgba(186, 172, 125, 255);
const INK: Color = Color::from_rgba(221, 221, 218, 255);
const LINK: Color = Color::from_rgba(117, 202, 239, 255);
const ROW: f32 = 28.0;
const LINE: f32 = 22.0;
const LOGO: &str = "misc/logo-bg.png~BLIT(misc/logo.png)";

struct Piece {
    span: usize,
    rect: Rect,
    word: Option<String>,
}
struct Article {
    pieces: Vec<Piece>,
    height: f32,
}

fn metrics<'a>(art: &'a Art, span: &Span) -> (Option<&'a Font>, u16) {
    match span {
        Span::Heading(_) => (art.font.as_ref(), 21),
        Span::Emphasis(_) => (art.italic.as_ref().or(art.font.as_ref()), 17),
        _ => (art.font.as_ref(), 17),
    }
}

fn topic_texture(root: &Path, src: &str) -> Option<Texture2D> {
    let name = Path::new(src.split('~').next()?);
    if !name.components().all(|c| matches!(c, Component::Normal(_))) {
        return None;
    }
    for prefix in ["images", "data/core/images"] {
        let file = Path::new(prefix).join(name);
        if root.join(&file).is_file() {
            return load_texture(root, file.to_str()?).ok();
        }
    }
    None
}

impl Article {
    fn layout(
        topic: &Topic,
        width: f32,
        root: &Path,
        art: &Art,
        textures: &mut HashMap<String, Option<Texture2D>>,
    ) -> Self {
        let (mut x, mut y) = (0.0, 0.0);
        let mut space = false;
        let mut pieces = Vec::new();
        for (index, span) in topic.spans.iter().enumerate() {
            let value = match span {
                Span::Image(name) => {
                    let size = if name == LOGO {
                        Some(art.logo.size())
                    } else {
                        textures
                            .entry(name.clone())
                            .or_insert_with(|| topic_texture(root, name))
                            .as_ref()
                            .map(Texture2D::size)
                    };
                    if let Some(size) = size {
                        if x > 0.0 {
                            y += LINE;
                        }
                        let size = size * (width / size.x).min(1.0);
                        pieces.push(Piece {
                            span: index,
                            rect: Rect::new(
                                if name == LOGO {
                                    (width - size.x) * 0.5
                                } else {
                                    0.0
                                },
                                y,
                                size.x,
                                size.y,
                            ),
                            word: None,
                        });
                        y += size.y + 4.0;
                        x = 0.0;
                        space = false;
                    }
                    continue;
                }
                Span::Text(s) | Span::Heading(s) | Span::Emphasis(s) | Span::Link(s, _) => s,
            };
            let (font, size) = metrics(art, span);
            let blank = measure_text(" ", font, size, 1.0).width;
            for (n, line) in value.split('\n').enumerate() {
                if n > 0 {
                    x = 0.0;
                    y += LINE;
                    space = false;
                }
                if line.starts_with(char::is_whitespace) {
                    space = true;
                }
                for (j, word) in line.split_whitespace().enumerate() {
                    let word = dialog::fit(font, word.to_owned(), width, size);
                    let advance = measure_text(&word, font, size, 1.0).width;
                    let gap = if x > 0.0 && (space || j > 0) {
                        blank
                    } else {
                        0.0
                    };
                    if x > 0.0 && x + gap + advance > width {
                        x = 0.0;
                        y += LINE;
                    } else {
                        x += gap;
                    }
                    pieces.push(Piece {
                        span: index,
                        rect: Rect::new(x, y, advance, LINE),
                        word: Some(word),
                    });
                    x += advance;
                    space = false;
                }
                if !line.is_empty() {
                    space = line.ends_with(char::is_whitespace);
                }
            }
        }
        Self {
            pieces,
            height: y + LINE,
        }
    }

    fn draw(
        &self,
        topic: &Topic,
        book: &Book,
        area: Rect,
        offset: f32,
        pointer: Vec2,
        art: &Art,
        textures: &HashMap<String, Option<Texture2D>>,
    ) -> Option<String> {
        let mut link = None;
        for piece in &self.pieces {
            let rect = Rect::new(
                area.x + piece.rect.x,
                area.y + piece.rect.y - offset,
                piece.rect.w,
                piece.rect.h,
            );
            if rect.bottom() <= area.y || rect.y >= area.bottom() {
                continue;
            }
            let span = &topic.spans[piece.span];
            if let Span::Image(src) = span {
                let paint = |texture: &Texture2D| {
                    let Some(clip) = rect.intersect(area) else {
                        return;
                    };
                    draw_texture_ex(
                        texture,
                        clip.x,
                        clip.y,
                        WHITE,
                        DrawTextureParams {
                            dest_size: Some(clip.size()),
                            source: Some(Rect::new(
                                (clip.x - rect.x) * texture.width() / rect.w,
                                (clip.y - rect.y) * texture.height() / rect.h,
                                clip.w * texture.width() / rect.w,
                                clip.h * texture.height() / rect.h,
                            )),
                            ..Default::default()
                        },
                    );
                };
                if src == LOGO {
                    paint(&art.logo_shadow);
                    paint(&art.logo);
                } else if let Some(Some(tex)) = textures.get(src) {
                    paint(tex);
                }
                continue;
            }
            if rect.y < area.y || rect.bottom() > area.bottom() {
                continue;
            }
            let (font, size) = metrics(art, span);
            let dest = match span {
                Span::Link(_, id) if book.topics.contains_key(id) => Some(id),
                _ => None,
            };
            let color = if dest.is_some() {
                LINK
            } else if matches!(span, Span::Heading(_)) {
                GOLD
            } else {
                INK
            };
            dialog::text(
                font,
                piece.word.as_deref().unwrap_or(""),
                rect.x,
                rect.y + 17.0,
                size,
                color,
            );
            if let Some(dest) = dest {
                draw_line(
                    rect.x,
                    rect.y + 19.0,
                    rect.right(),
                    rect.y + 19.0,
                    1.0,
                    color,
                );
                if rect.contains(pointer) {
                    link = Some(dest.clone());
                }
            }
        }
        link
    }
}

pub(super) async fn show(root: &Path, core: &[u8], art: &Art) -> Result<(), Box<dyn Error>> {
    let book = Book::parse(core)?;
    let mut current = book.initial().unwrap().to_owned();
    let mut opened = HashSet::new();
    book.reveal(&current, &mut opened);
    let mut history = Vec::new();
    let backdrop = load_texture(root, "images/dialogs/opaque-background.png")?;
    let icons = [
        load_texture(root, "images/help/topic.png")?,
        load_texture(root, "images/help/closed_section.png")?,
        load_texture(root, "images/help/open_section.png")?,
    ];
    let mut textures = HashMap::new();
    let mut cursor = 0usize;
    let mut start = 0usize;
    let mut offset = 0.0f32;
    let mut cached: Option<(String, f32, Article)> = None;
    loop {
        let scale = (screen_width() / 680.0)
            .min(screen_height() / 430.0)
            .clamp(0.01, 1.0);
        let (w, h) = (screen_width() / scale, screen_height() / scale);
        set_camera(&dialog::setup_camera(w, h));
        art.draw_background(w, h);
        let bounds = centered(
            vec2(w, h),
            vec2((w - 10.0).min(1200.0), (h - 10.0).min(700.0)),
        );
        tiled(bounds, &backdrop);
        draw_rectangle_lines(bounds.x, bounds.y, bounds.w, bounds.h, 3.0, GOLD);
        let left = bounds.x + 13.0;
        dialog::text(
            art.font.as_ref(),
            "Battle For Wesnoth Help",
            left,
            bounds.y + 36.0,
            28,
            GOLD,
        );
        let tree = Rect::new(
            left,
            bounds.y + 52.0,
            (bounds.w * 0.26).clamp(185.0, 300.0),
            bounds.h - 103.0,
        );
        let content = Rect::new(
            tree.right() + 19.0,
            tree.y,
            bounds.right() - tree.right() - 34.0,
            tree.h,
        );
        draw_rectangle(
            tree.x,
            tree.y,
            tree.w,
            tree.h,
            Color::from_rgba(5, 10, 15, 160),
        );
        draw_rectangle_lines(tree.x, tree.y, tree.w, tree.h, 1.0, GOLD);
        let body = Rect::new(content.x, content.y + 35.0, content.w, content.h - 36.0);
        let topic = &book.topics[&current];
        let heading = dialog::fit(art.font.as_ref(), topic.title.clone(), content.w, 24);
        dialog::text(
            art.font.as_ref(),
            &heading,
            content.x,
            content.y + 24.0,
            24,
            GOLD,
        );
        if !cached
            .as_ref()
            .is_some_and(|(id, width, _)| id == &current && *width == body.w)
        {
            cached = Some((
                current.clone(),
                body.w,
                Article::layout(topic, body.w, root, art, &mut textures),
            ));
        }
        let article = &cached.as_ref().unwrap().2;
        let point = Vec2::from(mouse_position()) / scale;
        let click = is_mouse_button_pressed(MouseButton::Left);
        let rows = book.rows(&opened);
        let visible = (tree.h / ROW).floor().max(1.0) as usize;
        cursor = cursor.min(rows.len().saturating_sub(1));
        let wheel = mouse_wheel().1.signum() as isize;
        if tree.contains(point) && wheel != 0 {
            start = start.saturating_add_signed(-wheel * 3);
        }
        let up = is_key_pressed(KeyCode::Up);
        let down = is_key_pressed(KeyCode::Down);
        if up || down {
            cursor = cursor
                .saturating_add_signed(if up { -1 } else { 1 })
                .min(rows.len().saturating_sub(1));
            start = start.min(cursor).max((cursor + 1).saturating_sub(visible));
        }
        start = start.min(rows.len().saturating_sub(visible));
        let mut activate = None;
        for (i, row) in rows.iter().enumerate().skip(start).take(visible) {
            let rect = Rect::new(
                tree.x + 1.0,
                tree.y + 1.0 + (i - start) as f32 * ROW,
                tree.w - 2.0,
                ROW,
            );
            if i == cursor || rect.contains(point) {
                draw_rectangle(
                    rect.x,
                    rect.y,
                    rect.w,
                    rect.h,
                    Color::from_rgba(47, 69, 84, 255),
                );
            }
            let x = rect.x + 7.0 + row.depth as f32 * 20.0;
            let icon = &icons[if row.folder {
                1 + usize::from(opened.contains(row.id))
            } else {
                0
            }];
            image(Rect::new(x, rect.y + 4.0, 20.0, 20.0), icon);
            let label = dialog::fit(
                art.font.as_ref(),
                row.title.into(),
                (rect.right() - x - 32.0).max(1.0),
                16,
            );
            dialog::text(art.font.as_ref(), &label, x + 26.0, rect.y + 20.0, 16, INK);
            if click && rect.contains(point) {
                cursor = i;
                activate = Some(i);
            }
        }
        let right = is_key_pressed(KeyCode::Right);
        let left_key = is_key_pressed(KeyCode::Left);
        if is_key_pressed(KeyCode::Enter) || right || left_key {
            activate = Some(cursor);
        }
        let mut go = None;
        if let Some(row) = activate.and_then(|i| rows.get(i)) {
            if row.folder {
                let was_open = opened.contains(row.id);
                if left_key || (was_open && !right) {
                    opened.remove(row.id);
                } else {
                    opened.insert(row.id.into());
                }
                let intro = format!("..{}", row.id);
                if book.topics.contains_key(&intro) {
                    go = Some(intro);
                }
            } else if !left_key {
                go = Some(row.id.into());
            }
        }
        if body.contains(point) {
            offset -= wheel as f32 * LINE * 3.0;
        }
        if is_key_pressed(KeyCode::PageDown) {
            offset += body.h - LINE;
        }
        if is_key_pressed(KeyCode::PageUp) {
            offset -= body.h - LINE;
        }
        offset = offset.clamp(0.0, (article.height - body.h).max(0.0));
        let clicked_link = article
            .draw(topic, &book, body, offset, point, art, &textures)
            .filter(|_| click);
        let mut sync_tree = clicked_link.is_some();
        if clicked_link.is_some() {
            go = clicked_link;
        }
        let close = Rect::new(bounds.right() - 120.0, bounds.bottom() - 40.0, 105.0, 29.0);
        let back = Rect::new(close.x - 93.0, close.y, 81.0, close.h);
        if !history.is_empty() {
            art.button(
                back,
                "Back",
                back.contains(point),
                back.contains(point) && is_mouse_button_down(MouseButton::Left),
            );
        }
        art.button(
            close,
            "Close",
            close.contains(point),
            close.contains(point) && is_mouse_button_down(MouseButton::Left),
        );
        set_default_camera();
        if is_key_pressed(KeyCode::Escape) || click && close.contains(point) {
            return Ok(());
        }
        if (is_key_pressed(KeyCode::Backspace) || click && back.contains(point))
            && let Some(previous) = history.pop()
        {
            current = previous;
            offset = 0.0;
            sync_tree = true;
        } else if let Some(id) = go
            && id != current
        {
            history.push(std::mem::replace(&mut current, id));
            offset = 0.0;
        }
        if sync_tree {
            book.reveal(&current, &mut opened);
            if let Some(target) = book
                .rows(&opened)
                .iter()
                .position(|r| r.id == current.trim_start_matches(".."))
            {
                cursor = target;
                start = start.min(cursor).max((cursor + 1).saturating_sub(visible));
            }
        }
        next_frame().await;
    }
}
