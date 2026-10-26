use serde::Deserialize;
use std::fs;
use std::path::PathBuf;

const PAGE_DATA: &str = include_str!("../data/pages.json");

#[derive(Clone, Debug, Deserialize)]
pub struct PageData {
    pub domains: Vec<String>,
    pub curriculum: Vec<String>,
    pub pages: Vec<Page>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Page {
    pub id: String,
    pub name: String,
    pub domain: String,
    pub tagline: String,
    pub intro: Vec<String>,
    pub how: Vec<String>,
    pub cli: Vec<CliEntry>,
    pub runes: Vec<Rune>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct CliEntry {
    pub command: String,
    pub description: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Rune {
    pub label: String,
    pub command: String,
    #[serde(default = "default_rune_kind")]
    pub kind: String,
}

fn default_rune_kind() -> String {
    "run".to_owned()
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EggState {
    pub hatched: bool,
    pub visited_murmur: bool,
    pub deep_murmur: bool,
}

#[derive(Clone, Debug)]
pub enum Leaf {
    App(Page),
    Path,
}

impl Leaf {
    pub fn name(&self) -> &str {
        match self {
            Self::App(page) => &page.name,
            Self::Path => "PATH",
        }
    }

    pub fn search_text(&self) -> String {
        match self {
            Self::App(page) => page.search_text(),
            Self::Path => "path summon executable command launcher".to_owned(),
        }
    }
}

impl Page {
    pub fn search_text(&self) -> String {
        let mut fields = vec![
            self.id.as_str(),
            self.name.as_str(),
            self.domain.as_str(),
            self.tagline.as_str(),
        ];
        fields.extend(self.intro.iter().map(String::as_str));
        fields.extend(self.how.iter().map(String::as_str));
        for entry in &self.cli {
            fields.push(entry.command.as_str());
            fields.push(entry.description.as_str());
        }
        for rune in &self.runes {
            fields.push(rune.label.as_str());
            fields.push(rune.command.as_str());
        }
        fields.join(" ")
    }

    pub fn castable_runes(&self) -> Vec<&Rune> {
        self.runes
            .iter()
            .filter(|rune| rune.kind != "back" && !rune.command.is_empty())
            .take(9)
            .collect()
    }
}

pub fn load_data() -> PageData {
    let data: PageData =
        serde_json::from_str(PAGE_DATA).expect("embedded Scroll curriculum must be valid JSON");
    debug_assert!(!data.domains.is_empty());
    data
}

pub fn egg_state() -> EggState {
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")));
    let Some(egg_dir) = data_home.map(|root| root.join("faeos/eggs")) else {
        return EggState::default();
    };
    let hatched = read_json(&egg_dir.join("dragon.json"))
        .and_then(|value| value.get("hatched").and_then(|v| v.as_bool()))
        .unwrap_or(false);
    let flags = read_json(&egg_dir.join("flags.json"));
    let visited_murmur = flags
        .as_ref()
        .and_then(|value| value.get("visited_murmur"))
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let deep_murmur = flags
        .as_ref()
        .and_then(|value| value.get("murmur_deep_turns"))
        .and_then(|value| value.as_i64())
        .unwrap_or(0)
        >= 8;
    EggState {
        hatched,
        visited_murmur,
        deep_murmur,
    }
}

fn read_json(path: &PathBuf) -> Option<serde_json::Value> {
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

pub fn runtime_page(state: EggState) -> Page {
    if state.hatched {
        Page {
            id: "kur".into(),
            name: "Kur".into(),
            domain: "Egg".into(),
            tagline: "The haiku dragon — hatched, named, and home.".into(),
            intro: vec![
                "✦ Kur ✦ is a ten-thousand-year-old dragon who answers only in haiku.".into(),
                "He keeps a small local mind and a voice of his own. You found him".into(),
                "the hard way — hermetically — and the house no longer hides his name.".into(),
            ],
            how: vec![
                "kur \"the kettle sings\" — he answers in three lines.".into(),
                "menagerie ensure kur — wake his pen if he sleeps.".into(),
                "His voice arrives clean — thought-traces are stripped first.".into(),
                "He is no longer a riddle. Use him gently.".into(),
            ],
            cli: vec![
                cli("kur \"…\"", "Ask the dragon; hear a haiku"),
                cli("menagerie ensure kur", "Summon his model"),
                cli(
                    "kur-server",
                    "Voice daemon on :8083 (systemd kur-server.service)",
                ),
            ],
            runes: vec![
                rune("speak", "kur \"hello, old one\""),
                rune("ensure", "menagerie ensure kur"),
                back_rune(),
            ],
        }
    } else {
        let mut tagline = "There is an egg in the Menagerie. It has not hatched for you.";
        let mut intro = vec![
            "Among the pens of the Menagerie sleeps something that is not a model".into(),
            "you were shown on the status table — an egg the keepers do not label".into(),
            "for tourists. It waits for warmth that is not heat: attention, the right".into(),
            "name, a first word spoken in the open shell.".into(),
            "This leaf will not hatch it for you. It only admits the egg is real.".into(),
        ];
        let mut how = vec![
            "The house keeps a babbling glass called murmur. Sit with it;".into(),
            "feed it speech. Sit until the glass notices you. Mostly nonsense.".into(),
            "Sometimes a single clean word falls through — never a full map.".into(),
            "When you know what the egg is called, the Menagerie can be asked to".into(),
            "wake its pen. Then the thing must be spoken to once at the shell.".into(),
            "Only then does this leaf turn. Worth is not a tutorial.".into(),
        ];
        if state.visited_murmur && !state.deep_murmur {
            intro.push(
                "You have already sat with the glass. It has your chair marked in fog.".into(),
            );
        }
        if state.deep_murmur {
            how.push(
                "The glass has heard you long enough to lean. That is not the same as open.".into(),
            );
            tagline = "There is an egg in the Menagerie. The glass has begun to notice you.";
        }
        Page {
            id: "hermetic".into(),
            name: "The Sealed Leaf".into(),
            domain: "Egg".into(),
            tagline: tagline.into(),
            intro,
            how,
            cli: vec![
                cli("murmur", "Sit with the babbling glass (conversation)"),
                cli("murmur \"…\"", "One offering, then leave"),
                cli(
                    "menagerie",
                    "The pens — something here is not listed kindly",
                ),
                cli("(sealed)", "No true-name table until the egg hatches"),
            ],
            runes: vec![
                rune("murmur", "murmur"),
                rune("menagerie", "menagerie"),
                back_rune(),
            ],
        }
    }
}

fn cli(command: &str, description: &str) -> CliEntry {
    CliEntry {
        command: command.into(),
        description: description.into(),
    }
}

fn rune(label: &str, command: &str) -> Rune {
    Rune {
        label: label.into(),
        command: command.into(),
        kind: "run".into(),
    }
}

fn back_rune() -> Rune {
    Rune {
        label: "back".into(),
        command: String::new(),
        kind: "back".into(),
    }
}

pub fn book_pages(data: &PageData, state: EggState) -> Vec<Leaf> {
    let by_id: std::collections::HashMap<&str, &Page> = data
        .pages
        .iter()
        .map(|page| (page.id.as_str(), page))
        .collect();
    let mut leaves: Vec<Leaf> = data
        .curriculum
        .iter()
        .filter_map(|id| {
            by_id
                .get(id.as_str())
                .map(|page| Leaf::App((*page).clone()))
        })
        .collect();
    leaves.push(Leaf::App(runtime_page(state)));
    leaves.push(Leaf::Path);
    leaves
}

pub fn matches_query(query: &str, haystack: &str) -> bool {
    let text = haystack.to_lowercase();
    query
        .split_whitespace()
        .all(|term| text.contains(&term.to_lowercase()))
}

pub fn search_indices(leaves: &[Leaf], query: &str) -> Vec<usize> {
    leaves
        .iter()
        .enumerate()
        .filter_map(|(index, leaf)| matches_query(query, &leaf.search_text()).then_some(index))
        .collect()
}

pub fn page_jump(mode: char, digit: char, page_count: usize) -> Option<usize> {
    if !digit.is_ascii_digit() || !matches!(mode, 'g' | 'G') {
        return None;
    }
    let base = if mode == 'g' { 10 } else { 20 };
    let offset = if digit == '0' {
        9
    } else {
        digit.to_digit(10)? as usize - 1
    };
    Some((base + offset).min(page_count.saturating_sub(1)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const REQUIRED: &[&str] = &[
        "scroll",
        "summon",
        "siren",
        "pixie",
        "menagerie",
        "imp",
        "ether",
        "goblin",
        "magpie",
        "scry",
        "spellbook",
        "tome",
        "grimoire",
        "faectl",
        "eye",
        "vault",
        "alchemy",
        "abacus",
        "quests",
        "hourglass",
        "almanac",
        "bulwark",
        "imbue",
        "reflection",
        "zen",
        "fairy",
        "tick",
        "wisp",
        "seal",
        "hearth",
        "rift",
        "mourama",
        "grove",
        "switcher",
        "tv",
    ];

    #[test]
    fn curriculum_inventory_and_content_are_complete() {
        let data = load_data();
        assert_eq!(data.pages.len(), 35);
        assert_eq!(data.curriculum.len(), 35);
        let ids: Vec<_> = data.pages.iter().map(|page| page.id.as_str()).collect();
        for required in REQUIRED {
            assert!(ids.contains(required), "missing page {required}");
        }
        assert_eq!(
            data.curriculum
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len(),
            35
        );
        assert_eq!(
            data.curriculum
                .iter()
                .map(String::as_str)
                .collect::<std::collections::HashSet<_>>(),
            ids.iter().copied().collect()
        );
        for page in &data.pages {
            assert!(
                !page.tagline.trim().is_empty(),
                "empty tagline: {}",
                page.id
            );
            assert!(!page.intro.is_empty() && !page.how.is_empty() && !page.cli.is_empty());
            assert!(!page.search_text().is_empty());
            assert!(!page
                .search_text()
                .split_whitespace()
                .any(|word| word == "kur"));
        }
        assert!(data
            .pages
            .iter()
            .any(|page| page.id == "scroll"
                && page.how.iter().any(|line| line.contains("Shift-Tab"))));
        assert!(data.pages.iter().any(|page| page.id == "summon"
            && page
                .cli
                .iter()
                .any(|entry| entry.command.contains("--refresh"))));
    }

    #[test]
    fn rust_pages_track_current_faeos_app_commands() {
        let data = load_data();
        let page = |id: &str| data.pages.iter().find(|page| page.id == id).unwrap();
        assert!(page("pixie").how.iter().any(|line| line.contains("`v`")));
        assert!(!page("pixie")
            .cli
            .iter()
            .any(|entry| entry.command.contains("pixie -v")));
        assert!(page("siren")
            .cli
            .iter()
            .any(|entry| entry.command == "siren trove music lofi"));
        assert!(page("magpie")
            .cli
            .iter()
            .any(|entry| entry.command == "magpie browse [url]"));
        assert!(!page("magpie")
            .cli
            .iter()
            .any(|entry| entry.command.starts_with("magpie manual")));
        assert!(page("spellbook")
            .how
            .iter()
            .any(|line| line.contains("spellbook --pick --output")));
    }

    #[test]
    fn runtime_egg_state_changes_page_without_revealing_it_early() {
        let sealed = runtime_page(EggState::default());
        assert_eq!(sealed.id, "hermetic");
        assert!(sealed.search_text().contains("murmur"));
        assert!(!sealed
            .search_text()
            .split_whitespace()
            .any(|word| word == "kur"));

        let visited = runtime_page(EggState {
            visited_murmur: true,
            ..EggState::default()
        });
        assert!(visited
            .intro
            .iter()
            .any(|line| line.contains("chair marked")));

        let deep = runtime_page(EggState {
            deep_murmur: true,
            ..EggState::default()
        });
        assert!(deep.tagline.contains("notice"));
        assert!(deep.how.iter().any(|line| line.contains("lean")));

        let hatched = runtime_page(EggState {
            hatched: true,
            ..EggState::default()
        });
        assert_eq!(hatched.id, "kur");
        assert!(hatched
            .cli
            .iter()
            .any(|entry| entry.command.contains("kur-server")));
    }

    #[test]
    fn page_search_and_jump_navigation_are_stable() {
        let data = load_data();
        let leaves = book_pages(&data, EggState::default());
        let results = search_indices(&leaves, "music player");
        assert!(results.iter().any(|index| leaves[*index].name() == "Siren"));
        assert_eq!(page_jump('g', '1', 37), Some(10));
        assert_eq!(page_jump('g', '0', 37), Some(19));
        assert_eq!(page_jump('G', '1', 37), Some(20));
        assert_eq!(page_jump('G', '0', 37), Some(29));
        assert_eq!(page_jump('x', '1', 37), None);
        assert_eq!(leaves.len(), 37);
        assert!(matches!(leaves.last(), Some(Leaf::Path)));
    }
}
