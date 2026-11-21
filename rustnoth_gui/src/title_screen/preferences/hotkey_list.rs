use super::{Art, Control, Layout, WidgetArt, BORDER, GOLD, LABEL, dialog};
use super::super::{hotkeys::{self, ACTIONS, Bindings}, load_texture};
use macroquad::prelude::*;
use std::{io, path::Path};

enum Prompt { Capture(usize), Replace { row: usize, key: String, old: usize }, Reset }

pub(super) struct List {
    filter: String,
    row: usize,
    sort_by_key: bool,
    reverse: bool,
    prompt: Option<Prompt>,
    icons: Vec<Option<Texture2D>>,
}

impl List {
    pub fn new(root: &Path) -> Self {
        let icons = ACTIONS.iter().map(|&(_, id, _)| {
            let filename = format!("images/icons/action/{id}_25.png");
            root.join(&filename).is_file().then(|| load_texture(root,&filename).ok()).flatten()
        }).collect();
        Self { filter: String::new(), row: 0, sort_by_key: false, reverse: false, prompt: None, icons }
    }
    pub fn modal(&self) -> bool { self.prompt.is_some() }

    pub fn draw(&mut self, bindings: &mut Bindings, art: &Art, skin: &WidgetArt, layout: &Layout,
            mouse: Vec2, focus: Control, activate: Option<Control>) -> io::Result<()> {
        use Control::*;
        let font = art.font.as_ref();
        let filter = layout.control(HotkeyFilter);
        let list = layout.control(HotkeyList);
        let highlight = Color::from_rgba(162,127,68,255);
        let field = |r: Rect, active: bool| {
            draw_rectangle(r.x,r.y,r.w,r.h,Color::from_rgba(0,0,0,120));
            draw_rectangle_lines(r.x,r.y,r.w,r.h,1.,if active {highlight} else {BORDER});
        };
        let prompt_open = self.modal();
        let chars: String = std::iter::from_fn(get_char_pressed).filter(|c| !c.is_control()).collect();
        if focus == HotkeyFilter && !prompt_open {
            if is_key_pressed(KeyCode::Backspace) { self.filter.pop(); }
            self.filter.push_str(&chars);
        }
        dialog::text(font,art.locale.ui("Filter:"),list.x,filter.y+20.,17,LABEL);
        dialog::text(font,art.locale.ui("Main Menu"),list.x+62.,filter.y+20.,16,GOLD);
        field(filter,focus==HotkeyFilter);
        let shown = if self.filter.is_empty() && focus!=HotkeyFilter { art.locale.ui("Search") } else { self.filter.as_str() };
        dialog::text(font,&dialog::fit(font,shown.to_owned(),filter.w-10.,16),filter.x+5.,filter.y+19.,16,LABEL);
        let mut rows: Vec<_> = (0..ACTIONS.len()).filter(|&i| {
            let name = art.locale.game(ACTIONS[i].2).to_lowercase();
            self.filter.split_whitespace().all(|s| name.contains(&s.to_lowercase()))
        }).collect();
        let click = !prompt_open && is_mouse_button_pressed(MouseButton::Left);
        let header = Rect::new(list.x,list.y,list.w,28.);
        if click && header.contains(mouse) {
            let keys = mouse.x >= list.x + list.w*0.49;
            self.reverse = self.sort_by_key == keys && !self.reverse;
            self.sort_by_key = keys;
        }
        rows.sort_by_cached_key(|&i| if self.sort_by_key { bindings.display(i) } else { art.locale.game(ACTIONS[i].2).to_lowercase() });
        if self.reverse { rows.reverse(); }
        let pos = rows.iter().position(|&i| i==self.row).unwrap_or(0);
        if !rows.is_empty() {
            let delta = if !prompt_open && focus==HotkeyList {
                i32::from(is_key_pressed(KeyCode::Down)) - i32::from(is_key_pressed(KeyCode::Up))
            } else {0};
            self.row = rows[(pos as i32 + delta).clamp(0,rows.len() as i32-1) as usize];
        }
        field(list,focus==HotkeyList);
        let mid = list.x + list.w*0.49;
        let mark = list.right()-26.;
        dialog::text(font,art.locale.ui("Action"),list.x+24.,list.y+20.,17,GOLD);
        dialog::text(font,art.locale.ui("Hotkey"),mid,list.y+20.,17,GOLD);
        dialog::text(font,"M",mark,list.y+20.,16,GOLD);
        for (n,&i) in rows.iter().enumerate() {
            let r = Rect::new(list.x+1.,list.y+28.+n as f32*27.,list.w-2.,27.);
            if click && r.contains(mouse) { self.row=i; }
            if i==self.row {
                draw_texture_ex(&skin.selected,r.x,r.y,WHITE,DrawTextureParams{dest_size:Some(r.size()),..Default::default()});
            }
            if let Some(icon)=&self.icons[i] {
                draw_texture_ex(icon,r.x+2.,r.y+4.,WHITE,DrawTextureParams{
                    source:Some(Rect::new(3.,3.,18.,18.)),dest_size:Some(vec2(18.,18.)),..Default::default()});
            }
            dialog::text(font,&dialog::fit(font,art.locale.game(ACTIONS[i].2).into(),mid-list.x-27.,15),r.x+23.,r.y+19.,15,LABEL);
            let keys = dialog::fit(font,bindings.display(i),mark-mid-6.,15);
            dialog::text(font,&keys,mid,r.y+19.,15,LABEL);
            dialog::text(font,"M",mark,r.y+19.,15,GOLD);
        }
        for (c,label) in [(HotkeyAdd,"Add Hotkey"),(HotkeyClear,"Clear Hotkey"),(HotkeyDefaults,"Defaults")] {
            let r = layout.control(c);
            art.button(r,art.locale.ui(label), r.contains(mouse)||focus==c, is_mouse_button_down(MouseButton::Left)&&r.contains(mouse));
        }
        if let Some(prompt) = self.prompt.take() {
            let box_rect = Rect::new(list.x+30.,list.y+131.,list.w-60.,125.);
            super::tiled(box_rect,&skin.backdrop);
            draw_rectangle_lines(box_rect.x,box_rect.y,box_rect.w,box_rect.h,2.,BORDER);
            let (title,details) = match &prompt {
                Prompt::Capture(i) => (art.locale.game(ACTIONS[*i].2).to_owned(), art.locale.ui("Press desired hotkey (Esc cancels)").to_owned()),
                Prompt::Replace{old,key,..} => (format!("{key} — {}",art.locale.game(ACTIONS[*old].2)),
                    "Reassign this hotkey?".into()),
                Prompt::Reset => (art.locale.ui("Defaults").to_owned(), "Restore the default hotkeys?".into()),
            };
            dialog::text(font,&title,box_rect.x+15.,box_rect.y+34.,18,GOLD);
            dialog::text(font,&details,box_rect.x+15.,box_rect.y+69.,16,LABEL);
            let cancel = Rect::new(box_rect.right()-119.,box_rect.bottom()-38.,106.,28.);
            let apply = Rect::new(cancel.x-116.,cancel.y,106.,28.);
            let released = is_mouse_button_released(MouseButton::Left);
            art.button(cancel,art.locale.ui("Cancel"),cancel.contains(mouse),is_mouse_button_down(MouseButton::Left)&&cancel.contains(mouse));
            if !matches!(prompt,Prompt::Capture(_)) {
                art.button(apply,art.locale.ui("OK"),apply.contains(mouse),is_mouse_button_down(MouseButton::Left)&&apply.contains(mouse));
            }
            if is_key_pressed(KeyCode::Escape) || released && cancel.contains(mouse) { return Ok(()); }
            let accepted = is_key_pressed(KeyCode::Enter) || released && apply.contains(mouse);
            match prompt {
                Prompt::Capture(row) => {
                    if let Some(key) = hotkeys::pressed() {
                        if let Some(old) = bindings.owner(&key) && old!=row {
                            self.prompt=Some(Prompt::Replace{row,key,old});
                        } else { bindings.add(row,&key)?; }
                    } else { self.prompt=Some(Prompt::Capture(row)); }
                }
                Prompt::Replace{row,key,..} if accepted => bindings.add(row,&key)?,
                Prompt::Reset if accepted => bindings.reset()?,
                pending => self.prompt=Some(pending),
            }
        } else {
            match activate {
                Some(HotkeyAdd) if !rows.is_empty() => self.prompt=Some(Prompt::Capture(self.row)),
                Some(HotkeyClear) if !rows.is_empty() => bindings.clear(self.row)?,
                Some(HotkeyDefaults) => self.prompt=Some(Prompt::Reset),
                _ => {}
            }
        }
        Ok(())
    }
}
