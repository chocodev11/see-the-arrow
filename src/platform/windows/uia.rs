//! UI Automation: the elements `--element` and `elements` search, and browser tabs.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Accessibility::{
    CUIAutomation, CUIAutomation8, IUIAutomation, IUIAutomation2, IUIAutomationCacheRequest,
    IUIAutomationElement, IUIAutomationInvokePattern, IUIAutomationSelectionItemPattern,
    IUIAutomationTreeWalker, TreeScope_Descendants, UIA_AutomationIdPropertyId,
    UIA_BoundingRectanglePropertyId, UIA_ControlTypePropertyId, UIA_InvokePatternId,
    UIA_IsEnabledPropertyId, UIA_NamePropertyId, UIA_SelectionItemPatternId,
    UIA_TabItemControlTypeId, UIA_ValueValuePropertyId,
};
use windows::core::{BSTR, Interface};

use super::apps::is_elevated;
use crate::model::display::Screens;
use crate::model::error::{Error, Result};
use crate::model::geom::Rect;
use crate::model::matcher::{AppInfo, ElementQuery, Node, WindowInfo};

const MAX_DEPTH: usize = 45;
const MAX_NODES: usize = 30_000;
const BUDGET: Duration = Duration::from_secs(6);
/// Per call: a hung app must not stall the search.
const TIMEOUT_MS: u32 = 500;

struct Automation {
    automation: IUIAutomation,
    cache: IUIAutomationCacheRequest,
    walker: IUIAutomationTreeWalker,
}

fn automation() -> Result<Automation> {
    // Already initialised in another mode on this thread is fine too.
    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    let failed = |_| Error::unresolvable("UI Automation is not available");
    let automation: IUIAutomation =
        unsafe { CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER) }
            .or_else(|_| unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) })
            .map_err(failed)?;
    if let Ok(timeouts) = automation.cast::<IUIAutomation2>() {
        let _ = unsafe { timeouts.SetConnectionTimeout(TIMEOUT_MS) };
        let _ = unsafe { timeouts.SetTransactionTimeout(TIMEOUT_MS) };
    }
    let cache = unsafe { automation.CreateCacheRequest() }.map_err(failed)?;
    for property in [
        UIA_NamePropertyId,
        UIA_ControlTypePropertyId,
        UIA_AutomationIdPropertyId,
        UIA_BoundingRectanglePropertyId,
        UIA_IsEnabledPropertyId,
        UIA_ValueValuePropertyId,
    ] {
        let _ = unsafe { cache.AddProperty(property) };
    }
    let walker = unsafe { automation.ControlViewWalker() }.map_err(failed)?;
    Ok(Automation {
        automation,
        cache,
        walker,
    })
}

/// The elements of `app`'s windows, breadth first, front window first. With `query`, the walk
/// stops at the first visible exact match.
pub fn elements(
    app: &AppInfo,
    windows: &[WindowInfo],
    query: Option<&ElementQuery>,
    screens: &Screens,
) -> Result<Vec<Node>> {
    if !is_elevated(None) && is_elevated(Some(app.pid)) {
        let name = app.name();
        return Err(Error::permission(format!(
            "Reading the UI elements of {name} needs administrator rights, because {name} runs as administrator: \
             restart {name} normally, or run your terminal as administrator. Run 'bigarrow doctor' to check."
        )));
    }
    let uia = automation()?;
    let start = Instant::now();
    let mut queue: VecDeque<(IUIAutomationElement, usize)> = windows
        .iter()
        .filter_map(|window| {
            unsafe {
                uia.automation
                    .ElementFromHandleBuildCache(HWND(window.handle as _), &uia.cache)
            }
            .ok()
        })
        .map(|element| (element, 0))
        .collect();
    let mut nodes = Vec::new();
    while let Some((element, depth)) = queue.pop_front() {
        if nodes.len() >= MAX_NODES || start.elapsed() > BUDGET {
            break;
        }
        let node = node(&element);
        let found = query
            .is_some_and(|query| query.score(&node) == 3 && node.is_visible(&screens.displays));
        nodes.push(node);
        if found {
            break;
        }
        if depth >= MAX_DEPTH {
            continue;
        }
        let mut child = unsafe {
            uia.walker
                .GetFirstChildElementBuildCache(&element, &uia.cache)
        }
        .ok();
        while let Some(current) = child {
            child = unsafe {
                uia.walker
                    .GetNextSiblingElementBuildCache(&current, &uia.cache)
            }
            .ok();
            queue.push_back((current, depth + 1));
        }
    }
    Ok(nodes)
}

fn node(element: &IUIAutomationElement) -> Node {
    let text = |value: windows::core::Result<BSTR>| {
        value
            .ok()
            .map(|text| text.to_string())
            .filter(|text| !text.is_empty())
    };
    let value = unsafe { element.GetCachedPropertyValue(UIA_ValueValuePropertyId) }
        .ok()
        .and_then(|variant: VARIANT| BSTR::try_from(&variant).ok())
        .map(|text| text.to_string())
        .filter(|text| !text.is_empty());
    // Win32 automation ids are control numbers like "1001", which only cause false matches.
    let id = text(unsafe { element.CachedAutomationId() })
        .filter(|id| !id.chars().all(|c| c.is_ascii_digit()));
    let frame = unsafe { element.CachedBoundingRectangle() }
        .ok()
        .map(|rect: RECT| {
            Rect::new(
                rect.left.into(),
                rect.top.into(),
                (rect.right - rect.left).into(),
                (rect.bottom - rect.top).into(),
            )
        });
    Node {
        enabled: unsafe { element.CachedIsEnabled() }
            .ok()
            .map(|enabled| enabled.as_bool()),
        frame: frame.filter(|rect| rect.width > 0.0 && rect.height > 0.0),
        id,
        name: text(unsafe { element.CachedName() }),
        role: control_type(unsafe { element.CachedControlType() }.map_or(0, |kind| kind.0))
            .to_owned(),
        value,
    }
}

/// UI Automation's control type names; the tab strip is "TabList" so `--role tab` means a tab.
fn control_type(id: i32) -> &'static str {
    const NAMES: [&str; 41] = [
        "Button",
        "Calendar",
        "CheckBox",
        "ComboBox",
        "Edit",
        "Hyperlink",
        "Image",
        "ListItem",
        "List",
        "Menu",
        "MenuBar",
        "MenuItem",
        "ProgressBar",
        "RadioButton",
        "ScrollBar",
        "Slider",
        "Spinner",
        "StatusBar",
        "TabList",
        "TabItem",
        "Text",
        "ToolBar",
        "ToolTip",
        "Tree",
        "TreeItem",
        "Custom",
        "Group",
        "Thumb",
        "DataGrid",
        "DataItem",
        "Document",
        "SplitButton",
        "Window",
        "Pane",
        "Header",
        "HeaderItem",
        "Table",
        "TitleBar",
        "Separator",
        "SemanticZoom",
        "AppBar",
    ];
    usize::try_from(id - 50_000)
        .ok()
        .and_then(|index| NAMES.get(index))
        .copied()
        .unwrap_or("Unknown")
}

/// Selects the tab whose name contains `title` in the window, e.g. a browser tab.
pub fn select_tab(window: u64, title: &str) -> bool {
    let Ok(uia) = automation() else { return false };
    let needle = title.to_lowercase();
    let found = || -> windows::core::Result<bool> {
        let root = unsafe { uia.automation.ElementFromHandle(HWND(window as _)) }?;
        let condition = unsafe {
            uia.automation.CreatePropertyCondition(
                UIA_ControlTypePropertyId,
                &VARIANT::from(UIA_TabItemControlTypeId.0),
            )
        }?;
        let tabs = unsafe { root.FindAll(TreeScope_Descendants, &condition) }?;
        for index in 0..unsafe { tabs.Length() }? {
            let tab = unsafe { tabs.GetElement(index) }?;
            if !unsafe { tab.CurrentName() }
                .is_ok_and(|name| name.to_string().to_lowercase().contains(&needle))
            {
                continue;
            }
            let selected = unsafe {
                tab.GetCurrentPatternAs::<IUIAutomationSelectionItemPattern>(
                    UIA_SelectionItemPatternId,
                )
            }
            .and_then(|pattern| unsafe { pattern.Select() })
            .or_else(|_| {
                unsafe {
                    tab.GetCurrentPatternAs::<IUIAutomationInvokePattern>(UIA_InvokePatternId)
                }
                .and_then(|pattern| unsafe { pattern.Invoke() })
            });
            return Ok(selected.is_ok());
        }
        Ok(false)
    };
    found().unwrap_or(false)
}
