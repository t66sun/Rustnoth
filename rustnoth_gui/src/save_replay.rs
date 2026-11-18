//! Player commands needed to reconstruct a desktop game from its original seed.
use crate::session::Session;
use engine_lite::{
    board::AttackChoice,
    fixed_match::Action,
    hex::Hex,
    wml::{Node, Value},
};
use std::io;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Player(Action, Option<String>),
    Computer,
    Dismiss,
}

pub fn set(node: &mut Node, key: &str, value: impl ToString) {
    node.attributes.insert(
        key.into(),
        Value {
            text: value.to_string(),
            translations: Vec::new(),
        },
    );
}

pub fn invalid(reason: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, reason)
}

impl Event {
    pub fn apply(&self, session: &mut Session<'_>) -> Result<(), Box<dyn std::error::Error>> {
        match self {
            Self::Player(action, choice) => {
                session.apply_with_advancement(action.clone(), choice.as_deref())?;
            }
            Self::Computer if session.play_computer_action()? => {}
            Self::Dismiss if session.dialogue().is_some() => session.dismiss_dialogue(),
            _ => return Err(invalid("Saved command does not match the current turn").into()),
        }
        Ok(())
    }

    pub fn encode(&self) -> Node {
        let mut node = Node::default();
        let at = |n: &mut Node, x, y, p: Hex| {
            set(n, x, p.x);
            set(n, y, p.y)
        };
        let kind = match self {
            Self::Computer => "computer",
            Self::Dismiss => "dismiss_dialogue",
            Self::Player(action, choice) => {
                if let Some(id) = choice {
                    set(&mut node, "advance", id);
                }
                match action {
                    Action::EndTurn => "end_turn",
                    Action::Move { from, to } => {
                        at(&mut node, "x", "y", *from);
                        at(&mut node, "to_x", "to_y", *to);
                        "move"
                    }
                    Action::Attack(a) => {
                        at(&mut node, "x", "y", a.from);
                        at(&mut node, "to_x", "to_y", a.to);
                        set(&mut node, "weapon", a.weapon);
                        "attack"
                    }
                    Action::Recruit { to, unit_type } => {
                        at(&mut node, "x", "y", *to);
                        set(&mut node, "type", unit_type);
                        "recruit"
                    }
                    Action::Recall { to, unit_id } => {
                        at(&mut node, "x", "y", *to);
                        set(&mut node, "id", unit_id);
                        "recall"
                    }
                    Action::Disband { unit_id } => {
                        set(&mut node, "id", unit_id);
                        "disband"
                    }
                }
            }
        };
        set(&mut node, "kind", kind);
        node
    }

    pub fn decode(node: &Node) -> io::Result<Self> {
        let field = |key: &str| {
            node.attribute(key)
                .ok_or_else(|| invalid("Missing saved command field"))
        };
        let hex = |x, y| -> io::Result<Hex> {
            let coordinate = |key| {
                field(key)?
                    .parse()
                    .map_err(|_| invalid("Invalid saved map coordinate"))
            };
            Ok(Hex {
                x: coordinate(x)?,
                y: coordinate(y)?,
            })
        };
        let action = match field("kind")? {
            "computer" => return Ok(Self::Computer),
            "dismiss_dialogue" => return Ok(Self::Dismiss),
            "end_turn" => Action::EndTurn,
            "move" => Action::Move {
                from: hex("x", "y")?,
                to: hex("to_x", "to_y")?,
            },
            "attack" => Action::Attack(AttackChoice {
                from: hex("x", "y")?,
                to: hex("to_x", "to_y")?,
                weapon: field("weapon")?
                    .parse()
                    .map_err(|_| invalid("Invalid saved weapon"))?,
            }),
            "recruit" => Action::Recruit {
                to: hex("x", "y")?,
                unit_type: field("type")?.into(),
            },
            "recall" => Action::Recall {
                to: hex("x", "y")?,
                unit_id: field("id")?.into(),
            },
            "disband" => Action::Disband {
                unit_id: field("id")?.into(),
            },
            _ => return Err(invalid("Unknown saved command")),
        };
        Ok(Self::Player(
            action,
            node.attribute("advance").map(str::to_owned),
        ))
    }
}
