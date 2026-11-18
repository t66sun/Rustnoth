use std::{borrow::Cow, error, fmt, io};

use crate::{
    map::{MapParseError, ScenarioMap},
    wml::Node,
};

/// Loads a fixed map from the scenario node, preferring inline `map_data`.
///
/// The caller resolves a `map_file` name in its Wesnoth data directories and
/// returns the corresponding map text.
pub fn load(
    scenario: &Node,
    read_map_file: impl FnOnce(&str) -> io::Result<String>,
) -> Result<ScenarioMap, ScenarioMapError> {
    let data = read_data(scenario, read_map_file)?;
    ScenarioMap::parse(&data).map_err(ScenarioMapError::Invalid)
}

pub fn read_data(
    scenario: &Node,
    read_map_file: impl FnOnce(&str) -> io::Result<String>,
) -> Result<Cow<'_, str>, ScenarioMapError> {
    if let Some(inline) = scenario.attribute("map_data").filter(|s| !s.is_empty()) {
        return Ok(Cow::Borrowed(inline));
    }

    let file = scenario
        .attribute("map_file")
        .filter(|s| !s.is_empty())
        .ok_or(ScenarioMapError::Missing)?;
    let contents = read_map_file(file).map_err(|source| ScenarioMapError::Read {
        file: file.to_owned(),
        source,
    })?;
    Ok(Cow::Owned(contents))
}

#[derive(Debug)]
pub enum ScenarioMapError {
    Missing,
    Read { file: String, source: io::Error },
    Invalid(MapParseError),
}

impl fmt::Display for ScenarioMapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing => f.write_str("scenario requires map_data or map_file"),
            Self::Read { file, source } => write!(f, "cannot read map {file}: {source}"),
            Self::Invalid(source) => write!(f, "invalid scenario map: {source}"),
        }
    }
}

impl error::Error for ScenarioMapError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::Missing => None,
            Self::Read { source, .. } => Some(source),
            Self::Invalid(source) => Some(source),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{hex::Hex, wml};

    const GRID: &str = "Xv,Xv,Xv\nXv,1 Kh,Xv\nXv,Xv,Xv";

    #[test]
    fn inline_map_takes_precedence() {
        let node = wml::parse(format!("map_file=ignored.map\nmap_data=\"{GRID}\"")).unwrap();
        let map = load(&node, |_| panic!("inline map should not read a file")).unwrap();
        assert_eq!(map.special_location("1"), Some(Hex { x: 0, y: 0 }));
    }

    #[test]
    fn empty_inline_map_falls_back_to_the_named_file() {
        let node = wml::parse("map_file=training.map\nmap_data=\"\"").unwrap();
        let map = load(&node, |name| {
            assert_eq!(name, "training.map");
            Ok(GRID.to_owned())
        })
        .unwrap();
        assert_eq!(map.terrain_at(Hex { x: 0, y: 0 }), Some("Kh"));
    }

    #[test]
    fn distinguishes_a_missing_source_from_a_map_error() {
        let absent = wml::parse("").unwrap();
        assert!(matches!(
            load(&absent, |_| unreachable!()),
            Err(ScenarioMapError::Missing)
        ));
        let malformed = wml::parse("map_file=unused.map\nmap_data=\"Xv,Xv\nXv\"").unwrap();
        assert!(matches!(
            load(&malformed, |_| unreachable!()),
            Err(ScenarioMapError::Invalid(MapParseError::NonRectangular { .. }))
        ));
    }
}
