//! Turns target specs into screen positions, on top of what the platform reports. The matching
//! rules live in `model::matcher`; this is the glue and the error messages.

use std::collections::{BTreeMap, HashSet};

use super::config::TargetSpec;
use crate::model::display::Screens;
use crate::model::error::{Error, Result};
use crate::model::geom::Point;
use crate::model::matcher::{
    self, AppInfo, ElementQuery, Node, WindowAnchor, WindowInfo, WindowQuery,
};
use crate::model::target::{ResolvedTarget, TargetShape};
use crate::platform::{self, WindowList};

/// Distance from the window edge for the corner anchors, in points.
const ANCHOR_INSET: f64 = 20.0;

pub fn resolve(spec: &TargetSpec, screens: &Screens) -> Result<ResolvedTarget> {
    match spec {
        TargetSpec::Coordinate(point, display) => Ok(ResolvedTarget::new(
            TargetShape::Point(screens.global(*point, *display)?),
            "coordinate",
        )),
        TargetSpec::Rect(rect, display) => {
            let origin = screens.global(rect.origin(), *display)?;
            let shape = TargetShape::Rect(crate::model::geom::Rect::new(
                origin.x,
                origin.y,
                rect.width,
                rect.height,
            ));
            Ok(ResolvedTarget::new(shape, "rect"))
        }
        TargetSpec::Mouse => Ok(ResolvedTarget::new(
            TargetShape::Point(platform::mouse_location()?),
            "mouse",
        )),
        TargetSpec::Window(query, anchor, name) => window(query, *anchor, name, screens),
        TargetSpec::Element(query, app) => element(query, app.as_deref(), screens),
    }
}

/// The app a query names. Several matches (two browser instances): the one the human sees.
pub fn find_app(name: &str, list: &WindowList, screens: &Screens) -> Result<AppInfo> {
    let matches = matcher::apps_matching(name, &list.apps);
    matcher::preferred(&matches, list.active, &list.windows, &screens.displays)
        .cloned()
        .ok_or_else(|| {
            Error::unresolvable(format!(
                "no running app matches '{name}'; apps with windows: {}",
                matcher::app_names(&list.apps).join(", ")
            ))
        })
}

pub fn frontmost_app(list: &WindowList) -> Result<AppInfo> {
    list.active
        .and_then(|pid| list.apps.iter().find(|app| app.pid == pid))
        .cloned()
        .ok_or_else(|| Error::unresolvable("no frontmost app, pass --app"))
}

fn window(
    query: &WindowQuery,
    anchor: WindowAnchor,
    anchor_name: &str,
    screens: &Screens,
) -> Result<ResolvedTarget> {
    let list = platform::window_list()?;
    let apps = matcher::apps_matching(&query.app, &list.apps);
    let Some(first) = apps.first() else {
        return Err(Error::unresolvable(format!(
            "no running app matches '{}'; apps with windows: {}",
            query.app,
            matcher::app_names(&list.apps).join(", ")
        )));
    };
    let pids: Vec<u32> = apps.iter().map(|app| app.pid).collect();
    let windows = matcher::windows_of(
        &pids,
        &list.windows,
        query.title.as_deref(),
        &screens.displays,
    );
    let Some(front) = windows.first() else {
        return Err(Error::unresolvable(match &query.title {
            Some(title) => format!(
                "{} has no window titled '*{title}*' on screen",
                first.name()
            ),
            None => format!("{} has no visible window on screen", first.name()),
        }));
    };
    let part = matcher::visible_part(&front.bounds, &screens.displays);
    let scale = screens
        .containing(part.center())
        .map_or(1.0, |display| display.scale);
    let point = anchor.point(
        part,
        ANCHOR_INSET * scale,
        platform::title_bar_center(scale),
    );
    let mut target = ResolvedTarget::new(TargetShape::Point(point), "window");
    target.detail = BTreeMap::from([
        ("app".to_owned(), first.name().to_owned()),
        ("anchor".to_owned(), anchor_name.to_owned()),
        ("windowCount".to_owned(), windows.len().to_string()),
    ]);
    if !front.title.is_empty() {
        target
            .detail
            .insert("title".to_owned(), front.title.clone());
    }
    Ok(target)
}

/// The app an element search runs in and its windows, front first.
pub fn element_scope(app: Option<&str>, screens: &Screens) -> Result<(AppInfo, Vec<WindowInfo>)> {
    let list = platform::window_list()?;
    let app = match app {
        Some(name) => find_app(name, &list, screens)?,
        None => frontmost_app(&list)?,
    };
    let windows = list
        .windows
        .into_iter()
        .filter(|window| window.pid == app.pid)
        .collect();
    Ok((app, windows))
}

fn element(query: &ElementQuery, app: Option<&str>, screens: &Screens) -> Result<ResolvedTarget> {
    let (app, windows) = element_scope(app, screens)?;
    let nodes = platform::elements(&app, &windows, Some(query), screens)?;
    let ranked = matcher::rank(&nodes, query, &screens.displays);
    let best = ranked.first().filter(|best| best.visible).ok_or_else(|| {
        let role = query
            .role
            .as_ref()
            .map(|role| format!(" with role {role}"))
            .unwrap_or_default();
        Error::unresolvable(format!(
            "no visible element labelled '{}'{role} in {} (searched {} elements)",
            query.text,
            app.name(),
            nodes.len()
        ))
    })?;
    let frame = best.node.frame.expect("visible elements have a frame");
    let mut target = ResolvedTarget::new(TargetShape::Rect(frame), "element");
    let alternatives: Vec<String> = ranked
        .iter()
        .skip(1)
        .take(5)
        .filter_map(|other| other.node.frame)
        .map(rect_text)
        .collect();
    target.detail = BTreeMap::from([
        ("app".to_owned(), app.name().to_owned()),
        ("matches".to_owned(), ranked.len().to_string()),
        ("role".to_owned(), best.node.role.clone()),
        (
            "name".to_owned(),
            best.node.label().unwrap_or_default().to_owned(),
        ),
        ("alternatives".to_owned(), alternatives.join(" ")),
    ]);
    Ok(target)
}

pub fn rect_text(rect: crate::model::geom::Rect) -> String {
    format!(
        "{},{},{},{}",
        rect.x as i64, rect.y as i64, rect.width as i64, rect.height as i64
    )
}

/// The UI elements `elements` lists: visible, labelled, optionally by role and label text.
pub fn list_elements(
    app: Option<&str>,
    role: Option<&str>,
    matching: Option<&str>,
    screens: &Screens,
) -> Result<Vec<Node>> {
    let (app, windows) = element_scope(app, screens)?;
    let role = role.map(matcher::canonical_role);
    let matching = matching.map(str::to_lowercase);
    let nodes = platform::elements(&app, &windows, None, screens)?;
    Ok(nodes
        .into_iter()
        .filter(|node| {
            node.is_visible(&screens.displays) && !node.texts().iter().all(String::is_empty)
        })
        .filter(|node| {
            role.as_ref()
                .is_none_or(|role| *role == matcher::canonical_role(&node.role))
        })
        .filter(|node| {
            matching
                .as_ref()
                .is_none_or(|text| node.texts().iter().any(|t| t.contains(text.as_str())))
        })
        .collect())
}

/// What `front` (and the raise before `point`) did.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Raised {
    pub app: String,
    pub frontmost: bool,
    pub pid: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window: Option<String>,
    #[serde(skip)]
    pub handle: u64,
}

/// Brings an app, and with `title` one of its windows or tabs, to the front. `frontmost` says
/// whether the system let it come to the front within 1.5 s.
pub fn raise(app: &str, title: Option<&str>, screens: &Screens) -> Result<Raised> {
    let list = platform::window_list()?;
    let app = find_app(app, &list, screens)?;
    let own: Vec<&WindowInfo> = list
        .windows
        .iter()
        .filter(|window| window.pid == app.pid)
        .collect();
    let chosen = match title {
        None => own.first().map(|window| window.handle),
        Some(title) => {
            let needle = title.to_lowercase();
            let titled = own
                .iter()
                .find(|window| window.title.to_lowercase().contains(&needle))
                .map(|window| window.handle);
            let tab = || {
                own.iter()
                    .map(|window| window.handle)
                    .find(|handle| platform::select_tab(*handle, title))
            };
            Some(titled.or_else(tab).ok_or_else(|| {
                Error::unresolvable(format!(
                    "no window or tab of pid {} has a title containing '{title}'",
                    app.pid
                ))
            })?)
        }
    };
    let (frontmost, handle) = platform::activate(app.pid, chosen);
    Ok(Raised {
        app: app.name().to_owned(),
        frontmost,
        pid: app.pid,
        window: title.map(str::to_owned),
        handle,
    })
}

/// True while another app's window covers `point`.
pub fn is_covered(home: &WindowQuery, point: Point, list: &WindowList) -> bool {
    let owners: HashSet<u32> = matcher::apps_matching(&home.app, &list.apps)
        .iter()
        .map(|app| app.pid)
        .collect();
    matcher::is_covered(point, &owners, &list.windows)
}
