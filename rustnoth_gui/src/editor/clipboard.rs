use super::{Command, EditableMap, Hex, History, Tool};
use std::collections::HashSet;

#[derive(Default)]
pub(super) struct Clipboard(Vec<(Hex, String)>);

fn sum(a: Hex, b: Hex) -> Hex {
    Hex { x: a.x + b.x, y: a.y + b.y + (a.x & b.x & 1) }
}

impl Clipboard {
    pub fn empty(&self) -> bool { self.0.is_empty() }

    pub fn copy(&mut self, doc: &EditableMap, selected: &HashSet<Hex>) {
        if selected.is_empty() { return; }
        // map_fragment averages the vector sum in offset coordinates, rounding toward zero.
        let total = selected.iter().copied().fold(Hex { x: 0, y: 0 }, sum);
        let count = selected.len() as i32;
        let cx = total.x / count;
        let neg = Hex { x: -cx, y: -(total.y / count) - (cx & 1) };
        self.0 = selected.iter().filter_map(|&at| {
            Some((sum(at, neg), doc.map().terrain_at(at)?.to_owned()))
        }).collect();
    }

    pub fn translated(&self, at: Hex) -> impl Iterator<Item=(Hex, &str)> {
        self.0.iter().map(move |(offset, code)| (sum(at, *offset), code.as_str()))
    }

    pub fn paste(&self, doc: &mut EditableMap, history: &mut History, at: Hex) -> bool {
        for (dest, code) in self.translated(at) { history.paint(doc, dest, code); }
        history.finish()
    }
}

#[derive(Clone, Copy)]
pub(super) struct SelectionState {
    count: usize,
    all: bool,
    pasting: bool,
    can_paste: bool,
}

impl SelectionState {
    pub fn new(doc: &EditableMap, history: &History, tool: Tool, clipboard: &Clipboard) -> Self {
        let map = doc.map();
        let count = history.selected.len();
        Self {
            count,
            all: count == ((map.width() + 2) * (map.height() + 2)) as usize,
            pasting: tool == Tool::Paste,
            can_paste: !clipboard.empty(),
        }
    }

    pub fn enabled(self, cmd: Command) -> bool {
        use Command::*;
        match cmd {
            Paste => self.can_paste,
            SelectAll | SelectNone => !self.pasting,
            SelectInverse => !self.pasting && self.count > 0 && !self.all,
            Copy | Cut | FillSelection => !self.pasting && self.count > 0,
            Shuffle => !self.pasting && self.count > 1,
            _ => true,
        }
    }

    pub fn selected(self, cmd: Command) -> bool {
        match cmd {
            Command::SelectAll => self.all,
            Command::SelectNone => self.count == 0,
            Command::Paste => self.pasting,
            _ => false,
        }
    }
}

pub(super) fn select(doc: &EditableMap, history: &mut History, cmd: Command) {
    let map = doc.map();
    let all = history.selected.len() == ((map.width() + 2) * (map.height() + 2)) as usize;
    for x in -1..=map.width() {
        for y in -1..=map.height() {
            let at = Hex { x, y };
            let add = match cmd {
                Command::SelectAll => !all,
                Command::SelectInverse => !history.selected.contains(&at),
                Command::SelectNone => false,
                _ => unreachable!(),
            };
            history.select(at, add);
        }
    }
    history.finish_selection();
}

pub(super) fn shuffle(doc: &mut EditableMap, history: &mut History) -> bool {
    let mut order: Vec<_> = history.selected.iter().copied().collect();
    order.sort_unstable();
    let mut shuffled = order.clone();
    for i in (1..shuffled.len()).rev() {
        let j = macroquad::rand::gen_range(0, i as i32 + 1) as usize;
        shuffled.swap(i, j);
    }
    for (a, b) in order.into_iter().zip(shuffled) {
        if a == b { continue; }
        let (Some(left), Some(right)) = (doc.map().terrain_at(a), doc.map().terrain_at(b)) else { continue };
        let (left, right) = (left.to_owned(), right.to_owned());
        history.paint(doc, a, &right);
        history.paint(doc, b, &left);
    }
    history.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clipboard_preserves_header_ids_and_stagger_across_clipped_pastes() {
        let mut doc = EditableMap::parse("Gg,Gg,Gg,Gg,Gg\nGg,1 Hh,Ww,Gg,Gg\nGg,Gg,Gg,Gg,Gg\nGg,Gg,Gg,Gg,Gg\n".into()).unwrap();
        let mut history = History::default();
        let mut clipboard = Clipboard::default();
        history.select(Hex { x: 0, y: 0 }, true);
        history.select(Hex { x: 1, y: 0 }, true);
        history.finish_selection();
        clipboard.copy(&doc, &history.selected);
        assert!(clipboard.paste(&mut doc, &mut history, Hex { x: 1, y: 0 }));
        assert_eq!(doc.map().terrain_at(Hex { x: 1, y: 0 }), Some("Hh"));
        assert_eq!(doc.map().terrain_at(Hex { x: 2, y: 1 }), Some("Ww"));
        assert_eq!(doc.map().special_location("1"), Some(Hex { x: 0, y: 0 }));
        assert!(history.apply(&mut doc, true));
        assert!(!doc.dirty());
        // Last stored column accepts only the first tile of the fragment.
        assert!(clipboard.paste(&mut doc, &mut history, Hex { x: 3, y: 0 }));
        assert_eq!(doc.map().terrain_at(Hex { x: 3, y: 0 }), Some("Hh"));
        assert!(history.apply(&mut doc, true));
        assert!(!doc.dirty());
    }

    #[test]
    fn select_all_toggles_the_stored_border_and_inverse_undo_restores_the_selection() {
        let doc = super::super::new_map();
        let mut h = History::default();
        let edge = Hex { x: -1, y: -1 };
        h.select(edge, true);
        h.finish_selection();
        select(&doc, &mut h, Command::SelectInverse);
        assert_eq!(h.selected.len(), 46 * 35 - 1);
        assert!(!h.selected.contains(&edge));
        select(&doc, &mut h, Command::SelectAll);
        assert!(h.selected.contains(&edge));
        let clipboard = Clipboard::default();
        let state = SelectionState::new(&doc, &h, Tool::Select, &clipboard);
        assert!(state.selected(Command::SelectAll) && !state.enabled(Command::SelectInverse));
        assert!(state.enabled(Command::Cut) && !state.enabled(Command::Paste));
        select(&doc, &mut h, Command::SelectAll);
        assert!(h.selected.is_empty());
        let mut doc = doc;
        h.apply(&mut doc, true);
        h.apply(&mut doc, true);
        h.apply(&mut doc, true);
        assert_eq!(h.selected, HashSet::from([edge]));
        assert!(!doc.dirty());
    }
}
