use super::*;
use super::super::{load_texture, panel_blur};

const GOLD: Color = Color::from_rgba(186, 172, 125, 255);
const LABEL: Color = Color::from_rgba(215, 215, 215, 255);
const DIM: Color = Color::from_rgba(128, 128, 128, 255);
const RIM: Color = Color::from_rgba(114, 79, 46, 255);
const BRIGHT: Color = Color::from_rgba(185, 158, 75, 255);
const DARK: Color = Color::from_rgba(0, 0, 0, 89);

pub(super) struct Layout { window: Rect }
impl Layout {
    pub fn new(view: Vec2) -> Self {
        let size = view.min(vec2(850., 800.));
        let origin = ((view-size)*0.5).floor();
        Self { window: Rect::new(origin.x, origin.y, size.x, size.y) }
    }
    fn x(&self) -> f32 { self.window.x + 15. }
    fn y(&self) -> f32 { self.window.y + 15. }
    fn right(&self) -> f32 { self.window.right() - 18. }
    fn bottom(&self) -> f32 { self.window.bottom() - 18. }
    pub fn filter(&self) -> Rect { Rect::new(self.right()-260., self.y(), 260., 30.) }
    pub fn folder(&self) -> Rect { Rect::new(self.filter().x-40., self.y(), 30., 30.) }
    pub fn list(&self) -> Rect {
        Rect::new(self.x(), self.y()+70., self.filter().x-self.x()-10., self.window.h-177.)
    }
    pub fn rows(&self) -> usize { (self.list().h / 41.).floor().max(1.) as usize }
    pub fn name_header(&self) -> Rect { Rect::new(self.x(), self.y()+40., self.list().w-150., 30.) }
    pub fn date_header(&self) -> Rect { Rect::new(self.name_header().right(), self.y()+40., 150., 30.) }
    pub fn load(&self) -> Rect { Rect::new(self.right()-294., self.bottom()-40., 172., 40.) }
    pub fn cancel(&self) -> Rect { Rect::new(self.right()-112., self.bottom()-33., 112., 26.) }
    pub fn delete(&self) -> Rect { Rect::new(self.x(), self.bottom()-33., 112., 26.) }
    fn check(&self, index: f32) -> Rect {
        let width = self.list().w / 3.;
        let x = match index as u8 { 0 => self.x(), 1 => self.x()+width+21., _ => self.x()+2.*width+10. };
        Rect::new(x, self.bottom()-70., width-10., 21.)
    }
    pub fn watch(&self) -> Rect { self.check(1.) }
}

fn fill(r: Rect, color: Color) { draw_rectangle(r.x, r.y, r.w, r.h, color); }
fn inset(r: Rect, by: f32) -> Rect { Rect::new(r.x+by,r.y+by,r.w-2.*by,r.h-2.*by) }
fn window(r: Rect, background: &Texture2D) {
    fill(r, Color::from_rgba(16, 22, 35, 255));
    fill(inset(r,1.), RIM);
    tiled(inset(r,2.), background);
}

pub(super) struct Widgets {
    background: Texture2D,
    selected: Texture2D,
    folder: [Texture2D; 3],
    frame: [Texture2D; 3],
    checks: [Texture2D; 4],
    search: Texture2D,
    gold: Texture2D,
    units: Texture2D,
}

impl Widgets {
    pub fn new(root: &Path) -> Result<Self, Box<dyn Error>> {
        let load = |path: &str| load_texture(root, &format!("images/{path}.png"));
        let states = |stem: &str| -> Result<_, Box<dyn Error>> {
            Ok([load(stem)?,load(&format!("{stem}-active"))?,load(&format!("{stem}-pressed"))?])
        };
        Ok(Self {
            background: load("dialogs/opaque-background")?, selected: load("dialogs/selection-background")?,
            frame: states("buttons/button_square/button_square_30")?, folder: states("icons/action/browse_25")?,
            checks: [load("buttons/checkbox")?,load("buttons/checkbox-active")?,
                load("buttons/checkbox-pressed")?,load("buttons/checkbox-active-pressed")?],
            search: load("icons/action/zoomdefault_25")?,
            gold: load_texture(root,"data/core/images/themes/gold.png")?,
            units: load_texture(root,"data/core/images/themes/units.png")?,
        })
    }

    pub fn frame(&self, l: &Layout, art: &Art, mouse: Vec2, filter: &str, editing: bool, all: bool) {
        window(l.window, &self.background);
        let font = art.font.as_ref();
        dialog::text(font, art.locale.ui("Load Game"), l.x(), l.y()+23., 22, GOLD);
        for (key, r) in [("Name", l.name_header()), ("Date",l.date_header())] {
            dialog::text(font, art.locale.ui(key), r.x+10., r.y+20., 15, GOLD);
        }
        let r = l.folder();
        let state = if r.contains(mouse) { if is_mouse_button_down(MouseButton::Left) { 2 } else { 1 } } else { 0 };
        image(r, &self.frame[state]);
        draw_texture(&self.folder[state],r.x+2., r.y+2.,WHITE);
        let r = l.filter();
        fill(r, DARK);
        draw_rectangle_lines(r.x+0.5,r.y+0.5,r.w-1.,r.h-1.,1.,if editing { BRIGHT } else { RIM });
        if !filter.is_empty() {
            let label = dialog::fit(font,filter.to_owned(),r.w-12.,17);
            if all { fill(Rect::new(r.x+5.,r.y+5.,measure_text(&label,font,17,1.).width,20.),Color::from_rgba(21,53,80,255)); }
            dialog::text(font,&label,r.x+5.,r.y+20.,17,LABEL);
        } else if !editing {
            draw_texture_ex(&self.search,r.x+3.,r.y+2.,WHITE,DrawTextureParams { flip_x:true,..Default::default() });
            dialog::text(font,art.locale.ui("Search"),r.x+31.,r.y+20.,15,DIM);
        }
    }

    pub fn row(&self, l: &Layout, art: &Art, line: usize, label: &str, date: &str, selected: bool) {
        let r = Rect::new(l.x(),l.list().y+line as f32*41.,l.list().w,41.);
        if selected { fill(r,BRIGHT); image(inset(r,1.),&self.selected); } else { fill(r,DARK); }
        let font = art.font.as_ref();
        dialog::text(font,&dialog::fit(font,label.into(),l.name_header().w-20.,17),r.x+10.,r.y+27.,17,LABEL);
        dialog::text(font,date,l.date_header().x+10.,r.y+27.,17,LABEL);
    }

    pub fn preview(&self, l: &Layout, art: &Art, preview: &Preview, date: &str) {
        let x = l.filter().x;
        let y = l.y()+40.;
        let font = art.font.as_ref();
        // The minimap widget retains its square grid cell even for rectangular maps.
        let size = (l.bottom()-y-225.).clamp(64.,260.);
        if let Some(tex) = &preview.map {
            image(Rect::new(x,y,tex.width()*size/260.,tex.height()*size/260.),tex);
        }
        let y = y+size+30.;
        dialog::text(font,&dialog::fit(font,preview.data.title.clone(),260.,20),x,y,20,GOLD);
        dialog::text(font,date,x,y+30.,15,LABEL);
        let summary = if preview.replay { art.locale.ui("Replay").to_owned() }
            else if let Some(turn) = preview.turn { format!("{} {turn}",art.locale.ui("Turn")) }
            else { String::new() };
        if preview.replay {
            dialog::text(font,&summary,x,y+53.,15,LABEL);
        } else {
            dialog::text(font,&dialog::fit(font,preview.mode_label.clone(),260.,15),x,y+53.,15,LABEL);
            dialog::text(font,&summary,x,y+76.,15,LABEL);
        }
        let line = y+if preview.replay { 75. } else { 98. };
        fill(Rect::new(x+5.,line,250.,1.),RIM);
        if let Some(leader) = &preview.leader {
            let top = line+15.;
            fill(Rect::new(x,top,260.,94.),DARK);
            if let Some(tex) = &leader.image {
                let factor = (72./tex.width()).min(72./tex.height()).min(1.);
                let size = tex.size()*factor;
                image(Rect::new(x+5.+(72.-size.x)*0.5,top+(94.-size.y)*0.5,size.x,size.y),tex);
            }
            dialog::text(font,&dialog::fit(font,leader.name.clone(),160.,17),x+92.,top+27.,17,LABEL);
            if let Some(gold) = leader.gold {
                draw_texture(&self.gold,x+92.,top+42.,WHITE);
                dialog::text(font,&gold.to_string(),x+118.,top+56.,15,GOLD);
            }
            draw_texture(&self.units,x+92.,top+66.,WHITE);
            let units = if let Some(reserve) = leader.reserve { format!("{} active, {} reserve",leader.active,reserve) }
                else { format!("{} active",leader.active) };
            dialog::text(font,&dialog::fit(font,units,142.,15),x+118.,top+80.,15,GOLD);
        }
    }

    pub fn invalid(&self, l: &Layout, art: &Art, message: &str) {
        dialog::wrap(art.font.as_ref(),message,l.filter().x,l.y()+85.,250.);
    }

    pub fn actions(&self, l: &Layout, art: &Art, mouse: Vec2, has_selection: bool, valid: bool, archive: bool, watch: bool) {
        let font = art.font.as_ref();
        for (i,label) in ["Change difficulty","Show replay","Cancel orders"].into_iter().enumerate() {
            let r = l.check(i as f32);
            // Rust checkpoints commit movement immediately and have no pending unit orders.
            let enabled = i == 1 && valid && !archive;
            let on = i == 1 && (watch || archive);
            let index = usize::from(on)*2+usize::from(enabled && r.contains(mouse));
            draw_texture(&self.checks[index],r.x,r.y,if enabled { WHITE } else { GRAY });
            dialog::text(font,art.locale.ui(label),r.x+25.,r.y+16.,15,if enabled { GOLD } else { DIM });
        }
        for (r,label,enabled) in [(l.delete(),"Delete",has_selection),(l.load(),"Load",valid),(l.cancel(),"Cancel",true)] {
            let hovered = enabled && r.contains(mouse);
            art.button(r,art.locale.ui(label),hovered,hovered && is_mouse_button_down(MouseButton::Left));
            if !enabled { fill(inset(r,2.),Color::from_rgba(38,38,38,145)); }
        }
    }

    pub fn prompt(&self, l: &Layout, art: &Art, mouse: Vec2, message: &str, confirm: bool) -> (Rect,Rect) {
        let center = l.window.center();
        let r = Rect::new(center.x-215.,center.y-78.,430.,156.);
        window(r,&self.background);
        dialog::wrap(art.font.as_ref(),message,r.x+15.,r.y+31.,400.);
        let no = Rect::new(r.right()-127.,r.bottom()-40.,112.,26.);
        let yes = if confirm { Rect { x:no.x-122.,..no } } else { no };
        if confirm { art.button(no,art.locale.ui("Cancel"),no.contains(mouse),false); }
        art.button(yes,art.locale.ui(if confirm {"Delete"} else {"OK"}),yes.contains(mouse),false);
        (yes,no)
    }
}

pub(super) async fn empty(root: &Path, art: &Art, behind: impl Fn(f32,f32)) -> Result<(), Box<dyn Error>> {
    let tile = load_texture(root,"images/dialogs/translucent65-background.png")?;
    let font = art.font.as_ref();
    let title = art.locale.ui("No Saved Games");
    let body = art.locale.ui("There are no saved games to load.");
    let mut background = None::<(Vec2,Texture2D)>;
    loop {
        let scale = (screen_width()/350.).min(screen_height()/130.).clamp(0.01,1.);
        let view = vec2(screen_width(),screen_height())/scale;
        set_camera(&dialog::setup_camera(view.x,view.y));
        behind(view.x,view.y);
        let w = 33.+measure_text(body,font,17,1.).width.ceil().max(measure_text(title,font,22,1.).width.ceil());
        let r = Rect::new(((view.x-w)*0.5).floor(),((view.y-91.)*0.5).floor(),w,91.);
        if background.as_ref().is_none_or(|(v,_)| *v != view) {
            let pixels = panel_blur::backdrop(&get_screen_data(),view,r,5);
            background = Some((view, Texture2D::from_image(&pixels)));
        }
        if let Some((_,blur)) = &background { image(r,blur); }
        draw_rectangle_lines(r.x+0.5,r.y+0.5,r.w-1.,r.h-1.,1.,Color::from_rgba(16,22,35,255));
        draw_rectangle_lines(r.x+1.5,r.y+1.5,r.w-3.,r.h-3.,1.,RIM);
        tiled(inset(r,2.),&tile);
        dialog::text(font,title,r.x+15.,r.y+37.,22,GOLD);
        dialog::text(font,body,r.x+15.,r.y+69.,17,LABEL);
        if is_mouse_button_released(MouseButton::Left) || is_key_pressed(KeyCode::Escape)
            || is_key_pressed(KeyCode::Enter) || is_key_pressed(KeyCode::Space) {
            set_default_camera(); return Ok(());
        }
        next_frame().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_load_dialog_bounds_at_the_desktop_reference_size() {
        let l = Layout::new(vec2(1200.,820.));
        assert_eq!(l.window, Rect::new(175.,10.,850.,800.));
        assert_eq!(l.filter(), Rect::new(747.,25.,260.,30.));
        assert_eq!(l.folder(), Rect::new(707.,25.,30.,30.));
        assert_eq!(l.load(), Rect::new(713.,752.,172.,40.));
        assert_eq!(l.date_header().x+10.,597.);
    }
}
