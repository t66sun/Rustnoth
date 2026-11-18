use super::{Hex, pattern};
use crate::render::ground::fingerprint;
use engine_lite::wml::Node;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

pub(super) struct Catalog {
    pub rules: Vec<Rule>,
    pub filters: Vec<String>,
    pub names: Vec<String>,
    pub images: Vec<Image>,
    pub files: HashMap<String, PathBuf>,
}
pub(super) struct Rule {
    pub cells: Vec<Cell>,
    pub probability: u32,
    pub hash: u32,
    pub precedence: i32,
    pub location: Option<Hex>,
    pub modulo: [i32; 2],
}
pub(super) struct Cell {
    pub offset: Hex,
    pub filter: usize,
    pub require: Vec<String>,
    pub forbid: Vec<String>,
    pub set: Vec<String>,
    pub parts: Vec<Part>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct Cut {
    pub x: i32,
    pub y: i32,
    pub center: Option<[i32; 2]>,
}
pub(super) struct Part {
    pub image: usize,
    pub layer: i32,
    pub base_y: i32,
    pub cut: Option<Cut>,
}
pub(super) struct Image {
    pub variants: Vec<Variant>,
    pub hash: u32,
}
pub(super) struct Variant {
    pub flags: Vec<String>,
    pub periods: Vec<String>,
    pub choices: Vec<Vec<(usize, u32)>>,
    pub random: i32,
}

struct Builder {
    data: Catalog,
    filter_ids: HashMap<String, usize>,
    name_ids: HashMap<String, usize>,
    image_ids: HashMap<(usize, usize), Option<usize>>,
}

pub(super) fn compile(nodes: &[Node], dirs: &[PathBuf]) -> Catalog {
    let mut files = HashMap::new();
    for dir in dirs {
        let base = dir.join("terrain");
        let mut todo = vec![base.clone()];
        while let Some(folder) = todo.pop() {
            let Ok(items) = std::fs::read_dir(&folder) else {
                continue;
            };
            for item in items.flatten() {
                let path = item.path();
                let Ok(kind) = item.file_type() else { continue };
                if kind.is_dir() {
                    todo.push(path);
                } else if kind.is_file()
                    && let Some(key) = path.strip_prefix(&base).ok().and_then(Path::to_str)
                {
                    files.entry(key.replace('\\', "/")).or_insert(path);
                }
            }
        }
    }
    let mut builder = Builder {
        data: Catalog {
            rules: Vec::new(),
            filters: Vec::new(),
            names: Vec::new(),
            images: Vec::new(),
            files,
        },
        filter_ids: HashMap::new(),
        name_ids: HashMap::new(),
        image_ids: HashMap::new(),
    };
    for node in nodes {
        builder.rule(node);
    }
    builder.data.rules.sort_by_key(|rule| rule.precedence);
    builder.data
}

fn attr<'a>(node: &'a Node, key: &str) -> &'a str {
    node.attribute(key).unwrap_or("")
}
fn number(node: &Node, key: &str, default: i32) -> i32 {
    attr(node, key).trim().parse().unwrap_or(default)
}
fn pair(value: &str) -> Option<[i32; 2]> {
    let (x, y) = value.split_once(',')?;
    Some([x.trim().parse().ok()?, y.trim().parse().ok()?])
}
pub(super) fn translate(at: Hex, delta: Hex) -> Hex {
    let stagger = if delta.x % 2 == 1 {
        at.x.rem_euclid(2)
    } else if delta.x % 2 == -1 {
        at.x.rem_euclid(2) - 1
    } else {
        0
    };
    Hex {
        x: at.x + delta.x,
        y: at.y + delta.y + stagger,
    }
}

fn rotate_hex(hex: Hex, steps: usize) -> Hex {
    let (mut q, mut r) = (hex.x, hex.y - hex.x.div_euclid(2));
    for _ in 0..steps {
        (q, r) = (-r, q + r);
    }
    Hex {
        x: q,
        y: r + q.div_euclid(2),
    }
}

fn normalize(cells: &mut [Cell]) {
    let Some(x) = cells.iter().map(|c| c.offset.x).min() else {
        return;
    };
    let mut y = cells
        .iter()
        .map(|c| 2 * c.offset.y + c.offset.x.rem_euclid(2))
        .min()
        .unwrap();
    if x % 2 == -1 && y % 2 != 0 {
        y += 2;
    }
    if x % 2 == 1 && y % 2 == 0 {
        y -= 2;
    }
    for c in cells {
        c.offset = translate(
            c.offset,
            Hex {
                x: -x,
                y: -((y - 1) / 2),
            },
        );
    }
}

struct Anchor<'a> {
    hex: Hex,
    tile: Option<&'a Node>,
}

fn anchors(node: &Node) -> Vec<Anchor<'_>> {
    let mut result = Vec::new();
    let mut numbered: BTreeMap<u32, Vec<Hex>> = BTreeMap::new();
    let mut rows = attr(node, "map")
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .peekable();
    let mut odd = rows.peek().is_some_and(|s| s.starts_with(','));
    let mut y = 0;
    for row in rows {
        let mut x = i32::from(odd);
        for token in row.split(',').map(str::trim).filter(|v| !v.is_empty()) {
            let hex = Hex { x, y };
            if token == "*" {
                result.push(Anchor { hex, tile: None });
            } else if let Ok(id) = token.parse() {
                numbered.entry(id).or_default().push(hex);
            }
            x += 2;
        }
        if odd {
            y += 1;
        }
        odd = !odd;
    }
    for (_, tile) in node.children.iter().filter(|(tag, _)| tag == "tile") {
        let mut locations = Vec::new();
        if let (Ok(x), Ok(y)) = (attr(tile, "x").parse(), attr(tile, "y").parse()) {
            locations.push(Hex { x, y });
        }
        if let Ok(pos) = attr(tile, "pos").parse()
            && let Some(hexes) = numbered.get(&pos)
        {
            locations.extend(hexes);
        }
        for hex in locations {
            result.push(Anchor {
                hex,
                tile: Some(tile),
            });
        }
    }
    result
}

impl Builder {
    fn rule(&mut self, node: &Node) {
        let cells = anchors(node);
        if cells.is_empty() {
            return;
        }
        let rotation = attr(node, "rotations");
        let names: Vec<_> = rotation.split(',').map(str::trim).collect();
        if !rotation.is_empty() && names.len() != 6 {
            return;
        }
        for angle in 0..names.len() {
            if names[angle] == "skip" {
                continue;
            }
            let mut out = Vec::new();
            let mut hash = 0u32;
            let mut invalid = false;
            for anchor in &cells {
                let filter = anchor.tile.and_then(|t| t.attribute("type")).unwrap_or("*");
                let filter = *self.filter_ids.entry(filter.into()).or_insert_with(|| {
                    let id = self.data.filters.len();
                    self.data.filters.push(filter.into());
                    id
                });
                let flags = |key: &str| {
                    [anchor.tile, Some(node)]
                        .into_iter()
                        .flatten()
                        .flat_map(|n| pattern::expand(attr(n, key)))
                        .map(|s| pattern::rotate(&s, angle, &names))
                        .collect::<Vec<_>>()
                };
                let mut forbid = flags("no_flag");
                let mut set = flags("set_flag");
                let both = flags("set_no_flag");
                forbid.extend(both.iter().cloned());
                set.extend(both);
                let mut parts = Vec::new();
                let mut add = |owner: &Node, global: bool| {
                    for (_, img) in owner.children.iter().filter(|(tag, _)| tag == "image") {
                        let Some(image) = self.image(img, angle, &names) else {
                            invalid = true;
                            return;
                        };
                        hash = hash.wrapping_add(self.data.images[image].hash);
                        if anchor.tile.is_some_and(|n| attr(n, "no_draw") == "yes") {
                            continue;
                        }
                        let offset = if global {
                            [
                                anchor.hex.x * 54,
                                anchor.hex.y * 72 + anchor.hex.x.rem_euclid(2) * 36,
                            ]
                        } else {
                            [0, 0]
                        };
                        let [bx, by] = pair(attr(img, "base"))
                            .map_or([36, 36], |[x, y]| [x - offset[0], y - offset[1]]);
                        let (mut px, mut py) = ((bx - 36) as f32, (by - 36) as f32);
                        for _ in 0..angle {
                            (px, py) = (0.5 * px - 0.75 * py, px + 0.5 * py);
                        }
                        parts.push(Part {
                            image,
                            layer: number(img, "layer", 0),
                            base_y: (py + 36.0) as i32,
                            cut: global.then(|| Cut {
                                x: 0,
                                y: 0,
                                center: pair(attr(img, "center")),
                            }),
                        });
                    }
                };
                add(node, true);
                if let Some(tile) = anchor.tile {
                    add(tile, false);
                }
                if invalid {
                    break;
                }
                out.push(Cell {
                    offset: rotate_hex(anchor.hex, angle),
                    filter,
                    require: flags("has_flag"),
                    forbid,
                    set,
                    parts,
                });
            }
            if invalid {
                continue;
            }
            if !rotation.is_empty() {
                normalize(&mut out);
            }
            for cell in &mut out {
                for part in &mut cell.parts {
                    if let Some(cut) = &mut part.cut {
                        cut.x = cell.offset.x * 54;
                        cut.y = cell.offset.y * 72 + cell.offset.x.rem_euclid(2) * 36;
                    }
                }
            }
            self.data.rules.push(Rule {
                cells: out,
                probability: number(node, "probability", 100) as u32,
                hash: if hash == 0 { 105533 } else { hash },
                precedence: number(node, "precedence", 0),
                location: node.attribute("x").and_then(|x| {
                    Some(Hex {
                        x: x.parse::<i32>().ok()? - 1,
                        y: node.attribute("y")?.parse::<i32>().ok()? - 1,
                    })
                }),
                modulo: [number(node, "mod_x", 0), number(node, "mod_y", 0)],
            });
        }
    }

    fn image(&mut self, img: &Node, angle: usize, names: &[&str]) -> Option<usize> {
        let key = (img as *const Node as usize, angle);
        if let Some(id) = self.image_ids.get(&key) {
            return *id;
        }
        let result = self.build_image(img, angle, names);
        self.image_ids.insert(key, result);
        result
    }

    fn build_image(&mut self, img: &Node, angle: usize, names: &[&str]) -> Option<usize> {
        let mut variants = Vec::new();
        let mut hash = 0u32;
        for variant in img
            .children
            .iter()
            .filter(|(tag, _)| tag == "variant")
            .map(|(_, n)| n)
            .chain(std::iter::once(img))
        {
            let template = pattern::rotate(attr(variant, "name"), angle, names);
            hash = hash.wrapping_add(fingerprint(&template));
            let mut choices = Vec::new();
            let vars = if template.contains("@V") {
                attr(img, "variations")
            } else {
                ""
            };
            for v in vars.split(';') {
                let mut frames = Vec::new();
                for frame in pattern::expand(&template.replace("@V", v)) {
                    let (name, time) = frame
                        .rsplit_once(':')
                        .and_then(|(n, t)| Some((n, t.trim().parse::<u32>().ok()?)))
                        .unwrap_or((&frame, 100));
                    let file = name.split('~').next().unwrap_or(name);
                    if !self.data.files.contains_key(file) {
                        continue;
                    }
                    let id = *self.name_ids.entry(name.into()).or_insert_with(|| {
                        let id = self.data.names.len();
                        self.data.names.push(name.into());
                        id
                    });
                    frames.push((id, time.max(1)));
                }
                if frames.is_empty() {
                    break;
                }
                choices.push(frames);
            }
            if choices.is_empty() {
                return None;
            }
            let random = match attr(variant, "random_start") {
                "no" | "false" => 0,
                v => v.parse().unwrap_or(-1),
            };
            variants.push(Variant {
                flags: pattern::expand(attr(variant, "has_flag")),
                periods: pattern::split(attr(variant, "tod"))
                    .into_iter()
                    .filter(|s| !s.is_empty())
                    .map(String::from)
                    .collect(),
                choices,
                random,
            });
        }
        let id = self.data.images.len();
        self.data.images.push(Image { variants, hash });
        Some(id)
    }
}
