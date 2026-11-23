//! Wesnoth achievement definitions and the status recorded by the original local profile.
use super::super::locale::Catalog;
use engine_lite::wml::Node;
use std::{collections::HashMap, io, path::Path};

pub(super) struct Group {
    pub id: String,
    pub name: String,
    pub total: usize,
    pub completed: usize,
    pub rows: Vec<Row>,
}

pub(super) struct Row {
    pub completed: bool,
    pub name: String,
    pub description: String,
    pub icon: String,
    pub progress: Option<(u32, u32)>,
    pub subs: Vec<(String, String, bool)>,
}

fn children<'a>(node: &'a Node, name: &'a str) -> impl Iterator<Item = &'a Node> {
    node.children.iter().filter_map(move |(key, child)| (key == name).then_some(child))
}

fn has_id(node: Option<&Node>, key: &str, id: &str) -> bool {
    node.and_then(|n| n.attribute(key)).unwrap_or("").split(',').any(|item| item.trim() == id)
}

pub(super) fn groups(config: &Node, profile: &Node, tutorial_progress: u32, root: &Path, language: &str) -> io::Result<Vec<Group>> {
    let mut domains = HashMap::new();
    let mut translate = |n: &Node, attr: &str| -> io::Result<String> {
        let Some(value) = n.attributes.get(attr) else { return Ok(String::new()); };
        let mut output = String::new();
        let mut offset = 0;
        for part in &value.translations {
            if !domains.contains_key(&part.domain) {
                domains.insert(part.domain.clone(), Catalog::installed(root, language, &part.domain)?);
            }
            output.push_str(&value.text[offset..part.bytes.start]);
            output.push_str(domains[&part.domain].text(&value.text[part.bytes.clone()]));
            offset = part.bytes.end;
        }
        output.push_str(&value.text[offset..]);
        Ok(output.replace("<i>", "").replace("</i>", "").replace("<b>", "").replace("</b>", ""))
    };

    let mut result = Vec::new();
    for group in children(config, "achievement_group") {
        let Some(key) = group.attribute("content_for").filter(|s| !s.is_empty()) else { continue };
        let status = children(profile, "achievements").find(|c| c.attribute("content_for") == Some(key));
        let mut group_view = Group { id: key.into(), name: translate(group, "display_name")?,
            total: 0, completed: 0, rows: Vec::new() };
        for row in children(group, "achievement") {
            let Some(id) = row.attribute("id").filter(|s| !s.is_empty() && !s.contains(',')) else { continue };
            group_view.total += 1;
            let earned = if key == "tutorial" && id == "completed" { tutorial_progress } else { 0 };
            let mut max: u32 = row.attribute("max_progress").and_then(|s| s.parse().ok()).unwrap_or(0);
            let complete = has_id(status, "ids", id) || max > 0 && earned >= max;
            if complete { group_view.completed += 1; }
            if !complete && matches!(row.attribute("hidden"), Some("yes" | "true" | "on" | "1")) { continue; }
            let partial = status.and_then(|s| children(s, "in_progress").find(|p| p.attribute("id") == Some(id)));
            let mut subs = Vec::new();
            for sub in children(row, "sub_achievement") {
                let Some(subid) = sub.attribute("id").filter(|v| !v.is_empty()) else { continue };
                max += 1;
                if subs.len() < 28 {
                    subs.push((sub.attribute("icon").unwrap_or("").to_owned(), translate(sub, "description")?,
                        complete || has_id(partial, "sub_ids", subid)));
                }
            }
            let current = partial.and_then(|p| p.attribute("progress_at")).and_then(|v| v.parse().ok()).unwrap_or(0);
            let attr = |normal, completed| if complete && row.attribute(completed).is_some_and(|v| !v.is_empty()) {
                completed
            } else { normal };
            group_view.rows.push(Row { completed: complete,
                name: translate(row, attr("name", "name_completed"))?,
                description: translate(row, attr("description", "description_completed"))?,
                icon: row.attribute(attr("icon", "icon_completed")).unwrap_or("").to_owned(),
                progress: (!complete && max > 0).then_some((earned.max(current), max)), subs });
        }
        result.push(group_view);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_lite::wml;
    #[test]
    fn hidden_rows_count_in_total_and_only_a_matching_profile_unlocks_them() {
        let definitions = wml::parse(r#"[achievement_group]
content_for=campaign
display_name=Campaign
[achievement]
id=secret
hidden=yes
name=Unknown
name_completed=Revealed
icon=unearned.png
icon_completed=earned.png
[/achievement]
[achievement]
id=steps
max_progress=2
[sub_achievement]
id=one
icon=item.png
[/sub_achievement]
[/achievement]
[/achievement_group]"#).unwrap();
        let profile = wml::parse(r#"[achievements]
content_for=campaign
ids=secret
[in_progress]
id=steps
progress_at=1
sub_ids=one
[/in_progress]
[/achievements]"#).unwrap();
        let fresh = groups(&definitions, &Node::default(), 0, Path::new("."), "C").unwrap().remove(0);
        assert_eq!((fresh.total, fresh.completed, fresh.rows.len(), fresh.rows[0].progress), (2, 0, 1, Some((0, 3))));
        let played = groups(&definitions, &profile, 0, Path::new("."), "C").unwrap().remove(0);
        assert_eq!((played.rows.len(), played.completed), (2, 1));
        assert_eq!((&*played.rows[0].name, &*played.rows[0].icon, played.rows[0].progress), ("Revealed", "earned.png", None));
        assert_eq!(played.rows[1].progress, Some((1, 3)));
        assert!(played.rows[1].subs[0].2);
        assert!(groups(&Node::default(), &profile, 0, Path::new("."), "C").unwrap().is_empty());
    }
}
