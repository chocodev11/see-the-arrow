//! AT-SPI over D-Bus: the elements `--element` and `elements` search, and tabs. The calls are
//! async; each search runs on zbus's own executor, no tokio.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use atspi::proxy::accessible::{AccessibleProxy, ObjectRefExt};
use atspi::proxy::proxy_ext::ProxyExt;
use atspi::{AccessibilityConnection, CoordType, Role, State};

use crate::model::display::Screens;
use crate::model::error::{Error, Result};
use crate::model::geom::Rect;
use crate::model::matcher::{AppInfo, ElementQuery, Node, WindowInfo};

const MAX_DEPTH: usize = 45;
const MAX_NODES: usize = 30_000;
const BUDGET: Duration = Duration::from_secs(6);

/// The accessibility bus answers.
pub fn available() -> bool {
    zbus::block_on(AccessibilityConnection::new()).is_ok()
}

/// Sets `org.a11y.Status.IsEnabled`, so toolkits started from now on expose their trees.
pub fn enable() {
    let _ = zbus::block_on(async {
        let session = zbus::Connection::session().await?;
        let status =
            zbus::Proxy::new(&session, "org.a11y.Bus", "/org/a11y/bus", "org.a11y.Status").await?;
        status
            .set_property("IsEnabled", true)
            .await
            .map_err(zbus::Error::from)
    });
}

pub fn elements(
    app: &AppInfo,
    windows: &[WindowInfo],
    query: Option<&ElementQuery>,
    screens: &Screens,
) -> Result<Vec<Node>> {
    if super::wayland() {
        return Err(super::needs_x11("Pointing at a UI element (--element)"));
    }
    enable();
    zbus::block_on(walk(app, windows, query, screens))
}

/// The root accessible of the app whose process is `pid`.
async fn app_root<'a>(
    connection: &'a AccessibilityConnection,
    pid: u32,
) -> Option<AccessibleProxy<'a>> {
    let registry = connection.root_accessible_on_registry().await.ok()?;
    let bus = zbus::fdo::DBusProxy::new(connection.connection())
        .await
        .ok()?;
    for child in registry.get_children().await.ok()? {
        let Some(name) = child.name() else { continue };
        let owner = bus
            .get_connection_unix_process_id(zbus::names::BusName::from(name.clone()))
            .await
            .ok();
        if owner == Some(pid) {
            return child
                .into_accessible_proxy(connection.connection())
                .await
                .ok();
        }
    }
    None
}

async fn walk(
    app: &AppInfo,
    windows: &[WindowInfo],
    query: Option<&ElementQuery>,
    screens: &Screens,
) -> Result<Vec<Node>> {
    let unavailable = || {
        Error::permission(super::missing_capability(
            "the accessibility bus",
            "Pointing at a UI element (--element)",
        ))
    };
    let connection = AccessibilityConnection::new()
        .await
        .map_err(|_| unavailable())?;
    let Some(root) = app_root(&connection, app.pid).await else {
        return Err(Error::permission(format!(
            "{} shows no UI elements on the accessibility bus: start it again after 'bigarrow doctor --request-accessibility' \
             (Chrome and Electron apps with --force-renderer-accessibility). Run 'bigarrow doctor' to check.",
            app.name()
        )));
    };
    let start = Instant::now();
    let mut nodes = Vec::new();
    let mut queue: VecDeque<(AccessibleProxy, usize, (f64, f64))> =
        VecDeque::from([(root, 0, (0.0, 0.0))]);
    while let Some((element, depth, offset)) = queue.pop_front() {
        if nodes.len() >= MAX_NODES || start.elapsed() > BUDGET {
            break;
        }
        let (node, own_offset) = node(&element, depth, offset, windows).await;
        let found = query
            .is_some_and(|query| query.score(&node) == 3 && node.is_visible(&screens.displays));
        if depth > 0 {
            nodes.push(node);
        }
        if found {
            break;
        }
        if depth >= MAX_DEPTH {
            continue;
        }
        for child in element.get_children().await.unwrap_or_default() {
            if let Ok(proxy) = child.into_accessible_proxy(connection.connection()).await {
                queue.push_back((proxy, depth + 1, own_offset));
            }
        }
    }
    Ok(nodes)
}

/// One element. GTK 4 reports positions relative to its window even when asked for screen
/// positions; a top-level window at 0,0 while its X window is elsewhere gets that window's
/// position added to everything inside it.
async fn node(
    element: &AccessibleProxy<'_>,
    depth: usize,
    offset: (f64, f64),
    windows: &[WindowInfo],
) -> (Node, (f64, f64)) {
    let role = element.get_role().await.unwrap_or(Role::Invalid);
    let states = element.get_state().await.unwrap_or_default();
    let shown = states.contains(State::Showing) && states.contains(State::Visible);
    let proxies = element.proxies().await.ok();
    let mut frame = None;
    if let Some(component) = match &proxies {
        Some(proxies) if shown => proxies.component().await.ok(),
        _ => None,
    } {
        frame = component
            .get_extents(CoordType::Screen)
            .await
            .ok()
            .map(|(x, y, width, height)| {
                Rect::new(x.into(), y.into(), width.into(), height.into())
            });
    }
    let mut own = offset;
    if depth == 1
        && windows.len() == 1
        && frame.is_some_and(|frame| frame.x == 0.0 && frame.y == 0.0)
        && windows[0].bounds.origin() != Default::default()
    {
        own = (windows[0].bounds.x, windows[0].bounds.y);
    }
    let frame = frame
        .map(|frame| frame.offset(own.0, own.1))
        .filter(|frame| frame.width > 0.0 && frame.height > 0.0);
    let value = match &proxies {
        Some(proxies) if matches!(role, Role::Entry | Role::PasswordText | Role::Text) => {
            match proxies.text().await {
                Ok(text) => text
                    .get_text(0, -1)
                    .await
                    .ok()
                    .filter(|text| !text.is_empty()),
                Err(_) => None,
            }
        }
        _ => None,
    };
    let name = element.name().await.ok().filter(|name| !name.is_empty());
    let id = element
        .accessible_id()
        .await
        .ok()
        .filter(|id| !id.is_empty());
    // AT-SPI's "text" is an editable text area.
    let role_name = if role == Role::Text {
        "text field"
    } else {
        role.name()
    };
    let enabled = states.contains(State::Enabled) || states.contains(State::Sensitive);
    (
        Node {
            enabled: Some(enabled),
            frame,
            id,
            name,
            role: role_name.to_owned(),
            value,
        },
        own,
    )
}

/// Selects the tab whose name contains `title` in the app's tab lists.
pub fn select_tab(pid: u32, title: &str) -> bool {
    let needle = title.to_lowercase();
    zbus::block_on(async {
        let Ok(connection) = AccessibilityConnection::new().await else {
            return false;
        };
        let Some(root) = app_root(&connection, pid).await else {
            return false;
        };
        let start = Instant::now();
        let mut queue = VecDeque::from([(root, 0usize)]);
        while let Some((element, depth)) = queue.pop_front() {
            if start.elapsed() > BUDGET {
                return false;
            }
            if element
                .get_role()
                .await
                .is_ok_and(|role| role == Role::PageTab)
                && element
                    .name()
                    .await
                    .is_ok_and(|name| name.to_lowercase().contains(&needle))
            {
                return select(&connection, &element).await;
            }
            if depth < 12 {
                for child in element.get_children().await.unwrap_or_default() {
                    if let Ok(proxy) = child.into_accessible_proxy(connection.connection()).await {
                        queue.push_back((proxy, depth + 1));
                    }
                }
            }
        }
        false
    })
}

/// Through the tab list's Selection, else the tab's first action.
async fn select(connection: &AccessibilityConnection, tab: &AccessibleProxy<'_>) -> bool {
    let index = tab.get_index_in_parent().await.unwrap_or(-1);
    if let Ok(parent) = tab.parent().await
        && let Ok(list) = parent.into_accessible_proxy(connection.connection()).await
        && let Ok(proxies) = list.proxies().await
        && let Ok(selection) = proxies.selection().await
        && selection.select_child(index).await.unwrap_or(false)
    {
        return true;
    }
    match tab.proxies().await {
        Ok(proxies) => match proxies.action().await {
            Ok(action) => action.do_action(0).await.unwrap_or(false),
            Err(_) => false,
        },
        Err(_) => false,
    }
}
