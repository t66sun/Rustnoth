//! Side-filtered campaign dialogue for the desktop player.
use super::FixedMatch;
use crate::{scenario_sides::bool_attribute, visibility::HexVisibility, wml::Node};

#[derive(Clone, Debug)]
pub enum Presentation {
    Dialogue(DialogueLine),
    TutorialHint(Node),
}

#[derive(Clone, Debug)]
pub struct DialogueLine {
    pub text: String,
    pub caption: Option<String>,
    pub image: Option<String>,
    pub speaker_type: Option<String>,
    pub speaker_side: Option<usize>,
    /// Explicit image_pos. Without it, the speaker's resolved profile controls alignment.
    pub portrait_right: Option<bool>,
    pub mirror: bool,
    pub second_image: Option<String>,
    pub second_mirror: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct Message {
    speaker: String,
    sides: Option<Vec<usize>>,
    line: DialogueLine,
}

impl Message {
    pub(crate) fn read(cfg: &Node) -> Option<Self> {
        // Options, text input and conditional dialogue require interactive WML.
        if !cfg.children.is_empty() || cfg.attributes.iter().any(|(k, v)|
            v.text.contains('$') && !(k == "image" && v.text == "$unit.profile" && cfg.attribute("speaker") == Some("narrator")))
            || cfg.attributes.keys().any(|k| !matches!(k.as_str(),
                "message" | "speaker" | "side_for" | "caption" | "image" | "image_pos" |
                "mirror" | "second_image" | "second_mirror" | "scroll" | "highlight" | "sound" | "voice"))
        { return None; }
        let speaker = cfg.attribute("speaker")?;
        if speaker.is_empty() { return None; }
        let text = cfg.attribute("message")?.trim();
        if text.is_empty() { return None; }
        let sides = cfg.attribute("side_for").map(|s| s.split(',').map(|n| n.trim().parse())
            .collect::<Result<Vec<usize>, _>>()).transpose().ok()?;
        let image = cfg.attribute("image").filter(|s| !s.is_empty()).map(str::to_owned);
        let portrait_right = match cfg.attribute("image_pos") {
            Some("left") => Some(false),
            Some("right") => Some(true),
            None => None,
            _ => return None,
        };
        Some(Self { speaker: speaker.to_owned(), sides, line: DialogueLine {
            text: text.to_owned(), caption: cfg.attribute("caption").map(str::to_owned),
            image, speaker_type: None, speaker_side: None, portrait_right, mirror: bool_attribute(cfg, "mirror", false),
            second_image: cfg.attribute("second_image").map(str::to_owned),
            second_mirror: bool_attribute(cfg, "second_mirror", false),
        }})
    }
}

impl FixedMatch<'_> {
    pub fn tutorial_print(&self) -> Option<&Node> { self.tutorial_print.as_ref() }

    pub fn tutorial_end_turn_denial(&self) -> Option<&Node> { self.tutorial_end_turn_denial.as_ref() }

    /// Show a prepared WML message using the ordinary speaker and audience checks.
    pub fn present_message(&mut self, config: &Node) {
        if let Some(message) = Message::read(config) { self.queue_dialogue(message, [None; 2]); }
    }

    pub(super) fn queue_dialogue(&mut self, message: Message, event_units: [Option<u64>; 2]) {
        let mut line = message.line;
        let speaker = if line.image.as_deref() == Some("$unit.profile") {
            let Some((position, unit)) = self.scenario.board.units()
                .find(|(_, unit)| Some(unit.serial) == event_units[0]) else { return };
            line.image = unit.portrait.clone();
            line.speaker_type = Some(unit.unit_type.id.clone());
            line.speaker_side = Some(unit.side);
            Some((position, unit.side))
        } else if message.speaker == "narrator" {
            None
        } else {
            let Some((position, unit)) = self.scenario.board.units().find(|(_, unit)| match message.speaker.as_str() {
                "unit" => Some(unit.serial) == event_units[0],
                "second_unit" => Some(unit.serial) == event_units[1],
                id => unit.id.as_deref() == Some(id),
            }) else { return };
            line.caption = line.caption.or_else(|| unit.display_name.clone().filter(|name| !name.is_empty()));
            if line.image.is_none() && line.second_image.as_deref().unwrap_or("").is_empty() {
                line.image = unit.portrait.clone();
            }
            line.speaker_type = Some(unit.unit_type.id.clone());
            line.speaker_side = Some(unit.side);
            Some((position, unit.side))
        };
        let audience = (1..=self.scenario.sides.len()).filter(|side| {
            message.sides.as_ref().is_none_or(|sides| sides.contains(side))
                && speaker.is_none_or(|(position, owner)| owner == *side
                    || (self.effective_vision(*side).at(position) == Some(HexVisibility::Visible)
                    && self.unit_visible_on_known_hex(*side, position)))
        }).collect::<Vec<_>>();
        if !audience.is_empty() { self.presentation.push((audience, Presentation::Dialogue(line))); }
    }

    /// Consume the last command's presentation queue for the local human view.
    pub fn take_dialogue(&mut self, viewer: usize) -> Vec<DialogueLine> {
        self.take_presentation(viewer).into_iter().filter_map(|item| match item {
            Presentation::Dialogue(line) => Some(line),
            Presentation::TutorialHint(_) => None,
        }).collect()
    }

    pub fn take_presentation(&mut self, viewer: usize) -> Vec<Presentation> {
        std::mem::take(&mut self.presentation).into_iter().filter_map(|(audience, item)|
            audience.contains(&viewer).then_some(item)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_preserves_both_authored_portraits_and_independent_mirroring() {
        let node = crate::wml::parse("speaker=narrator\nmessage=Halt!\nimage=elves/archer.png~RIGHT()\nimage_pos=left\nmirror=yes\nsecond_image=humans/bowman.png\nsecond_mirror=no\n").unwrap();
        let line = Message::read(&node).unwrap().line;
        assert_eq!(line.image.as_deref(), Some("elves/archer.png~RIGHT()"));
        assert_eq!(line.portrait_right, Some(false));
        assert!(line.mirror);
        assert_eq!(line.second_image.as_deref(), Some("humans/bowman.png"));
        assert!(!line.second_mirror);
    }
}
