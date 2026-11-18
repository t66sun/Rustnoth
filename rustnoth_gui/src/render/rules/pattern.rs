//! TerrainGraphicsWML lists, image ranges and rotation placeholders.
pub(super) fn split(input: &str) -> Vec<&str> {
    let mut depth = 0;
    let mut begin = 0;
    let mut parts = Vec::new();
    for (at, ch) in input.char_indices() {
        match ch {
            '[' | '(' => depth += 1,
            ']' | ')' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(input[begin..at].trim());
                begin = at + 1;
            }
            _ => {}
        }
    }
    parts.push(input[begin..].trim());
    parts
}

fn values(input: &str) -> Vec<String> {
    let mut out = Vec::new();
    for word in input.split(',').map(str::trim) {
        if let Some((lo, hi)) = word.split_once('~')
            && let (Ok(first), Ok(last)) = (lo.parse::<i32>(), hi.parse::<i32>())
        {
            let width = if lo.starts_with('0') { lo.len() } else { 0 };
            let step = if last >= first { 1 } else { -1 };
            for i in 0..=(last - first).unsigned_abs().min(1024) {
                out.push(format!("{:0width$}", first + step * i as i32));
            }
        } else if let Some((value, times)) = word.split_once('*')
            && let Ok(times) = times.parse::<usize>()
        {
            out.extend(std::iter::repeat_n(value.to_owned(), times.min(1024)));
        } else {
            out.push(word.to_owned());
        }
    }
    out
}

/// Parallel expansions such as [1~4,3,2].png:[700*6] zip by frame number.
pub(in crate::render) fn expand(input: &str) -> Vec<String> {
    let mut out = Vec::new();
    for part in split(input).into_iter().filter(|s| !s.is_empty()) {
        let mut columns = vec![String::new()];
        let mut rest = part;
        while let Some((prefix, suffix)) = rest.split_once('[') {
            let Some((list, tail)) = suffix.split_once(']') else {
                break;
            };
            let entries = values(list);
            if columns.len() == 1 {
                columns.resize(entries.len(), columns[0].clone());
            }
            for (i, column) in columns.iter_mut().enumerate() {
                column.push_str(prefix);
                if let Some(value) = entries.get(i).or(entries.last()) {
                    column.push_str(value);
                }
            }
            rest = tail;
        }
        for mut column in columns {
            column.push_str(rest);
            out.push(column);
        }
    }
    out
}

pub(super) fn rotate(text: &str, rotation: usize, names: &[&str]) -> String {
    if !text.contains("@R") || names.len() != 6 {
        return text.to_owned();
    }
    let mut value = text.to_owned();
    for n in 0..6 {
        value = value.replace(&format!("@R{n}"), names[(n + rotation) % 6]);
    }
    value
}

pub(in crate::render) fn matches(filter: &str, terrain: &str) -> bool {
    let (base, overlay) = terrain.split_once('^').unwrap_or((terrain, ""));
    let layer = |value: &str, pattern: &str| {
        pattern
            .strip_suffix('*')
            .map_or_else(|| value == pattern, |prefix| value.starts_with(prefix))
    };
    let mut include = true;
    for word in filter.split(',').map(str::trim) {
        if word == "!" {
            include = !include;
            continue;
        }
        let (b, o) = word.split_once('^').unwrap_or((word, "*"));
        if word == "*" || layer(base, b) && layer(overlay, o) {
            return include;
        }
    }
    !include
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn range_frames_remain_synchronized_and_terrain_negations_toggle() {
        assert_eq!(
            expand("kelp[01~03,02].png:[100*3,200]"),
            [
                "kelp01.png:100",
                "kelp02.png:100",
                "kelp03.png:100",
                "kelp02.png:200"
            ]
        );
        assert!(matches("!,W*,!,G*", "Gg^Fp"));
        assert!(!matches("!,W*,!,G*", "Ww"));
        assert!(matches("!,W*", "Re"));
        assert!(!matches("Gg^", "Gg^Vh"));
    }
}
