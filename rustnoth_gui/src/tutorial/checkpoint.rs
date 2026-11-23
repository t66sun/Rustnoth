//! Rebuild the scripted lesson around the same referee commands used by campaign saves.
use super::{Hero, Lesson, opening, put};
use engine_lite::wml::Node;
use rustnoth_gui::{save_replay::Event, session::Session};
use std::error::Error;

#[cfg(test)]
mod tests;

pub(crate) struct SavedHints {
    movement_seen: bool,
    weapon_selection_seen: bool,
    show_undo: bool,
    floating_hint: String,
}

impl Lesson {
    pub(crate) fn checkpoint(&self, floating_hint: &str) -> Node {
        let mut saved = Node::default();
        put(&mut saved, "hero", match self.hero { Hero::Konrad => "Konrad", Hero::Lisar => "Lisar" });
        put(&mut saved, "hint", floating_hint);
        if self.selection_hint.is_none() { put(&mut saved, "movement_seen", "yes"); }
        if self.weapons.select_student.is_none() { put(&mut saved, "weapon_selection_seen", "yes"); }
        if self.show_undo_prompt { put(&mut saved, "show_undo", "yes"); }
        saved.children.push(("scenario".into(), self.authored.clone()));
        saved
    }

    pub(crate) fn from_checkpoint(saved: &Node) -> Result<(Self, SavedHints), Box<dyn Error>> {
        let hero = match saved.attribute("hero") {
            Some("Konrad") => Hero::Konrad,
            Some("Lisar") => Hero::Lisar,
            _ => return Err("Unknown Battle Training student in save".into()),
        };
        let mut scenario = opening::children(saved, "scenario").next()
            .ok_or("Missing authored Battle Training scenario in save")?.clone();
        let lesson = opening::prepare(&mut scenario, hero)?;
        let hints = SavedHints {
            movement_seen: saved.attribute("movement_seen") == Some("yes"),
            weapon_selection_seen: saved.attribute("weapon_selection_seen") == Some("yes"),
            show_undo: saved.attribute("show_undo") == Some("yes"),
            floating_hint: saved.attribute("hint").unwrap_or_default().into(),
        };
        Ok((lesson, hints))
    }

    pub(crate) fn resume(&mut self, session: &mut Session<'_>, events: &[Event], hints: SavedHints,
        root: &std::path::Path, locale: &mut crate::title_screen::locale::Locale)
        -> Result<(), Box<dyn Error>>
    {
        self.start(session, root, locale);
        for event in events {
            session.dismiss_local_dialogue();
            let weapon = match event {
                Event::Player(action, ..) => self.weapon_hint(session, action),
                _ => None,
            };
            event.apply(session)?;
            match event {
                Event::Player(..) => self.after_player(session, weapon),
                Event::Computer(_) => self.after_computer(session),
                _ => {}
            }
        }
        session.dismiss_local_dialogue();
        // Clicks and undo update presentation without adding referee commands.
        if hints.movement_seen { self.selection_hint = None; }
        if hints.weapon_selection_seen { self.weapons.select_student = None; }
        self.show_undo_prompt = hints.show_undo;
        session.set_tutorial_hint(&opening::node(&[("message", &hints.floating_hint)]));
        Ok(())
    }
}
