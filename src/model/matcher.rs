//! Matches `--window`/`--app`/`--element` queries against what the platform reports. Pure.

use std::collections::HashSet;

use serde::Serialize;

use super::display::Display;
use super::error::{Error, Result};
use super::geom::{Point, Rect};

/// A running app with at least one window. `names[0]` is the name shown to humans; every name
/// (product name, executable, app id) can match.
#[derive(Debug, Clone, PartialEq)]
pub struct AppInfo {
    pub pid: u32,
    pub names: Vec<String>,
}

impl AppInfo {
    pub fn name(&self) -> &str {
        self.names.first().map_or("", String::as_str)
    }
}

/// One normal top-level window, in front-to-back order, global input units.
#[derive(Debug, Clone, PartialEq)]
pub struct WindowInfo {
    pub pid: u32,
    pub bounds: Rect,
    pub title: String,
    /// The platform's window handle.
    pub handle: u64,
}

/// `--window "App Name[:title substring]"`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowQuery {
    pub app: String,
    pub title: Option<String>,
}

impl WindowQuery {
    pub fn parse(raw: &str) -> Result<Self> {
        let (app, title) = raw.split_once(':').unwrap_or((raw, ""));
        let app = app.trim();
        if app.is_empty() {
            return Err(Error::bad_input(
                "--window needs an app name, example: 'Google Chrome'",
            ));
        }
        let title = title.trim();
        Ok(Self {
            app: app.to_owned(),
            title: (!title.is_empty()).then(|| title.to_owned()),
        })
    }
}

/// Which point inside a window the arrow points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowAnchor {
    Center,
    Title,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl WindowAnchor {
    const NAMES: [(&'static str, Self); 6] = [
        ("center", Self::Center),
        ("title", Self::Title),
        ("top-left", Self::TopLeft),
        ("top-right", Self::TopRight),
        ("bottom-left", Self::BottomLeft),
        ("bottom-right", Self::BottomRight),
    ];

    pub fn parse(raw: &str) -> Result<Self> {
        let key = raw.to_lowercase();
        Self::NAMES
            .iter()
            .find(|(name, _)| *name == key)
            .map(|(_, anchor)| *anchor)
            .ok_or_else(|| {
                let names: Vec<&str> = Self::NAMES.iter().map(|(name, _)| *name).collect();
                Error::bad_input(format!(
                    "--anchor '{raw}' is not one of {}",
                    names.join(", ")
                ))
            })
    }

    /// `inset` is the distance from the edges for corners, `title` the title bar's centre line,
    /// both in the window's units.
    pub fn point(self, rect: Rect, inset: f64, title: f64) -> Point {
        match self {
            Self::Center => rect.center(),
            Self::Title => Point::new(rect.mid_x(), rect.min_y() + title),
            Self::TopLeft => Point::new(rect.min_x() + inset, rect.min_y() + inset),
            Self::TopRight => Point::new(rect.max_x() - inset, rect.min_y() + inset),
            Self::BottomLeft => Point::new(rect.min_x() + inset, rect.max_y() - inset),
            Self::BottomRight => Point::new(rect.max_x() - inset, rect.max_y() - inset),
        }
    }
}

/// Exact name first, then prefix, then substring. Case-insensitive.
pub fn apps_matching<'a>(query: &str, apps: &'a [AppInfo]) -> Vec<&'a AppInfo> {
    let needle = query.to_lowercase();
    let tiers: [&dyn Fn(&str) -> bool; 3] = [
        &|name| name == needle,
        &|name| name.starts_with(&needle),
        &|name| name.contains(&needle),
    ];
    for tier in tiers {
        let matched: Vec<&AppInfo> = apps
            .iter()
            .filter(|app| app.names.iter().any(|name| tier(&name.to_lowercase())))
            .collect();
        if !matched.is_empty() {
            return matched;
        }
    }
    Vec::new()
}

/// The smallest part of a window, in points per side, that must lie on a display.
const MINIMUM_VISIBLE_SIDE: f64 = 40.0;

/// Windows of the given pids that show on a display, front to back, optionally by title.
pub fn windows_of<'a>(
    pids: &[u32],
    windows: &'a [WindowInfo],
    title: Option<&str>,
    displays: &[Display],
) -> Vec<&'a WindowInfo> {
    let title = title.map(str::to_lowercase);
    windows
        .iter()
        .filter(|window| pids.contains(&window.pid) && is_on_screen(&window.bounds, displays))
        .filter(|window| {
            title
                .as_ref()
                .is_none_or(|title| window.title.to_lowercase().contains(title.as_str()))
        })
        .collect()
}

/// The part of a window the human can see: its largest overlap with a display.
pub fn visible_part(bounds: &Rect, displays: &[Display]) -> Rect {
    displays
        .iter()
        .filter_map(|display| display.frame.intersection(bounds))
        .max_by(|a, b| a.area().total_cmp(&b.area()))
        .unwrap_or(*bounds)
}

fn is_on_screen(bounds: &Rect, displays: &[Display]) -> bool {
    displays.iter().any(|display| {
        let side = MINIMUM_VISIBLE_SIDE * display.scale;
        display
            .frame
            .intersection(bounds)
            .is_some_and(|part| part.width >= side && part.height >= side)
    })
}

/// When several running apps share a name (two browser instances), the one the human sees wins:
/// the active app, else the app owning the frontmost visible window, else the first match.
pub fn preferred<'a>(
    matches: &[&'a AppInfo],
    active: Option<u32>,
    windows: &[WindowInfo],
    displays: &[Display],
) -> Option<&'a AppInfo> {
    if let Some(app) = matches.iter().find(|app| Some(app.pid) == active) {
        return Some(app);
    }
    let front = windows.iter().find(|window| {
        matches.iter().any(|app| app.pid == window.pid) && is_on_screen(&window.bounds, displays)
    });
    front
        .and_then(|window| matches.iter().find(|app| app.pid == window.pid))
        .or(matches.first())
        .copied()
}

/// Names of the apps with windows, for the "unknown app" message.
pub fn app_names(apps: &[AppInfo]) -> Vec<String> {
    let mut names: Vec<String> = apps.iter().map(|app| app.name().to_owned()).collect();
    names.sort();
    names.dedup();
    names
}

/// True when the frontmost window under `point` belongs to another app than `owners`. Nothing
/// under the point (taskbar, desktop) does not count as covered.
pub fn is_covered(point: Point, owners: &HashSet<u32>, windows: &[WindowInfo]) -> bool {
    windows
        .iter()
        .find(|window| window.bounds.contains(point))
        .is_some_and(|top| !owners.contains(&top.pid))
}

/// One UI element as the platform's accessibility API reports it. `frame` is global input units.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Node {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "frame_array"
    )]
    pub frame: Option<Rect>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
}

fn frame_array<S: serde::Serializer>(
    frame: &Option<Rect>,
    serializer: S,
) -> std::result::Result<S::Ok, S::Error> {
    serde::Serialize::serialize(&frame.map(|rect| rect.to_array()), serializer)
}

impl Node {
    pub fn texts(&self) -> Vec<String> {
        [&self.name, &self.value, &self.id]
            .into_iter()
            .flatten()
            .map(|text| text.to_lowercase())
            .collect()
    }

    /// The first non-empty of name and value.
    pub fn label(&self) -> Option<&str> {
        [&self.name, &self.value]
            .into_iter()
            .flatten()
            .map(String::as_str)
            .find(|text| !text.is_empty())
    }

    pub fn is_visible(&self, displays: &[Display]) -> bool {
        self.frame.is_some_and(|frame| {
            frame.width > 0.0
                && frame.height > 0.0
                && displays
                    .iter()
                    .any(|display| display.frame.intersects(&frame))
        })
    }
}

/// Role names that mean the same thing on macOS, Windows (UI Automation) and Linux (AT-SPI),
/// after lowercasing and dropping `ax`, spaces, `_` and `-`.
const ROLE_ALIASES: [(&str, &str); 19] = [
    ("pushbutton", "button"),
    ("togglebutton", "checkbox"),
    ("switch", "checkbox"),
    ("edit", "textfield"),
    ("entry", "textfield"),
    ("textarea", "textfield"),
    ("passwordtext", "textfield"),
    ("label", "statictext"),
    ("text", "statictext"),
    ("hyperlink", "link"),
    ("splitbutton", "menubutton"),
    ("popupbutton", "combobox"),
    ("menubaritem", "menuitem"),
    ("tabitem", "tab"),
    ("pagetab", "tab"),
    ("panel", "group"),
    ("pane", "group"),
    ("frame", "window"),
    ("icon", "image"),
];

/// `AXButton`, `button`, `Button` and `push button` are the same role.
pub fn canonical_role(raw: &str) -> String {
    let lower = raw.to_lowercase();
    let squeezed: String = lower
        .strip_prefix("ax")
        .unwrap_or(&lower)
        .chars()
        .filter(|c| !matches!(c, ' ' | '_' | '-'))
        .collect();
    ROLE_ALIASES
        .iter()
        .find(|(alias, _)| *alias == squeezed)
        .map_or(squeezed, |(_, canonical)| (*canonical).to_owned())
}

/// `--element 'Save' [--role button]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElementQuery {
    pub text: String,
    /// Canonical role.
    pub role: Option<String>,
}

impl ElementQuery {
    pub fn new(text: &str, role: Option<&str>) -> Result<Self> {
        if text.trim().is_empty() {
            return Err(Error::bad_input(
                "--element needs a label, example: --element 'Reload'",
            ));
        }
        Ok(Self {
            text: text.to_owned(),
            role: role.map(canonical_role),
        })
    }

    /// 3 exact, 2 prefix, 1 substring, 0 no match.
    pub fn score(&self, node: &Node) -> u8 {
        if self
            .role
            .as_ref()
            .is_some_and(|role| *role != canonical_role(&node.role))
        {
            return 0;
        }
        let needle = self.text.to_lowercase();
        let texts = node.texts();
        if texts.contains(&needle) {
            3
        } else if texts.iter().any(|text| text.starts_with(&needle)) {
            2
        } else if texts.iter().any(|text| text.contains(&needle)) {
            1
        } else {
            0
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Match<'a> {
    pub node: &'a Node,
    pub score: u8,
    pub visible: bool,
    pub order: usize,
}

/// Ranks matching elements: visible first, then best text match, then enabled, then tree order.
pub fn rank<'a>(nodes: &'a [Node], query: &ElementQuery, displays: &[Display]) -> Vec<Match<'a>> {
    let mut matches: Vec<Match> = nodes
        .iter()
        .enumerate()
        .filter_map(|(order, node)| {
            let score = query.score(node);
            (score > 0).then(|| Match {
                node,
                score,
                visible: node.is_visible(displays),
                order,
            })
        })
        .collect();
    matches.sort_by(|a, b| {
        b.visible
            .cmp(&a.visible)
            .then(b.score.cmp(&a.score))
            .then(
                b.node
                    .enabled
                    .unwrap_or(true)
                    .cmp(&a.node.enabled.unwrap_or(true)),
            )
            .then(a.order.cmp(&b.order))
    });
    matches
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(pid: u32, names: &[&str]) -> AppInfo {
        AppInfo {
            pid,
            names: names.iter().map(|name| (*name).to_owned()).collect(),
        }
    }

    fn display() -> Display {
        Display {
            index: 0,
            id: 1,
            frame: Rect::new(0.0, 0.0, 1920.0, 1080.0),
            visible: Rect::new(0.0, 0.0, 1920.0, 1040.0),
            scale: 1.0,
        }
    }

    fn window(pid: u32, bounds: Rect, title: &str) -> WindowInfo {
        WindowInfo {
            pid,
            bounds,
            title: title.to_owned(),
            handle: 0,
        }
    }

    #[test]
    fn app_names_match_in_tiers() {
        let apps = [
            app(1, &["Windows Terminal", "WindowsTerminal"]),
            app(2, &["Terminal Helper"]),
            app(3, &["Google Chrome", "chrome"]),
        ];
        assert_eq!(
            apps_matching("windows terminal", &apps)
                .iter()
                .map(|a| a.pid)
                .collect::<Vec<_>>(),
            vec![1]
        );
        assert_eq!(
            apps_matching("terminal", &apps)
                .iter()
                .map(|a| a.pid)
                .collect::<Vec<_>>(),
            vec![2]
        );
        assert_eq!(
            apps_matching("CHROME", &apps)
                .iter()
                .map(|a| a.pid)
                .collect::<Vec<_>>(),
            vec![3]
        );
        assert!(apps_matching("firefox", &apps).is_empty());
    }

    #[test]
    fn window_queries_split_at_the_first_colon_and_anchors_sit_inside() {
        let query = WindowQuery::parse("Google Chrome:Inbox: 3 new").unwrap();
        assert_eq!(query.app, "Google Chrome");
        assert_eq!(query.title.as_deref(), Some("Inbox: 3 new"));
        assert_eq!(WindowQuery::parse("Notepad").unwrap().title, None);
        assert!(WindowQuery::parse(":title").is_err());
        let rect = Rect::new(100.0, 50.0, 800.0, 600.0);
        assert_eq!(
            WindowAnchor::Center.point(rect, 20.0, 14.0),
            Point::new(500.0, 350.0)
        );
        assert_eq!(
            WindowAnchor::Title.point(rect, 20.0, 14.0),
            Point::new(500.0, 64.0)
        );
        assert_eq!(
            WindowAnchor::BottomRight.point(rect, 20.0, 14.0),
            Point::new(880.0, 630.0)
        );
        assert!(WindowAnchor::parse("middle").is_err());
    }

    #[test]
    fn coverage_and_window_filters() {
        let windows = [
            window(7, Rect::new(0.0, 0.0, 500.0, 500.0), "Other"),
            window(1, Rect::new(0.0, 0.0, 1000.0, 800.0), "Inbox - Mail"),
            window(1, Rect::new(5000.0, 5000.0, 400.0, 400.0), "Parked"),
        ];
        let owners = HashSet::from([1]);
        assert!(is_covered(Point::new(100.0, 100.0), &owners, &windows));
        assert!(!is_covered(Point::new(700.0, 600.0), &owners, &windows));
        assert!(!is_covered(Point::new(1500.0, 1000.0), &owners, &windows));
        let shown = windows_of(&[1], &windows, Some("INBOX"), &[display()]);
        assert_eq!(shown.len(), 1);
        assert!(windows_of(&[1], &windows, Some("Parked"), &[display()]).is_empty());
    }

    #[test]
    fn elements_rank_visible_then_score_then_enabled() {
        let node = |name: &str, role: &str, frame: Option<Rect>, enabled: Option<bool>| Node {
            enabled,
            frame,
            id: None,
            name: Some(name.to_owned()),
            role: role.to_owned(),
            value: None,
        };
        let on = Some(Rect::new(10.0, 10.0, 50.0, 20.0));
        let nodes = [
            node("Save", "Button", None, Some(true)),
            node("Save as", "Button", on, Some(true)),
            node("Save", "Button", on, Some(false)),
            node("Save", "MenuItem", on, Some(true)),
        ];
        let query = ElementQuery::new("save", Some("button")).unwrap();
        let ranked = rank(&nodes, &query, &[display()]);
        assert_eq!(
            ranked.iter().map(|m| m.order).collect::<Vec<_>>(),
            vec![2, 1, 0]
        );
        assert_eq!(canonical_role("AXRadioButton"), "radiobutton");
        assert_eq!(canonical_role("push button"), "button");
        assert_eq!(canonical_role("TabItem"), canonical_role("page tab"));
        assert!(ElementQuery::new("  ", None).is_err());
    }
}
