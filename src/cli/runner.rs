//! `point` and `start`: raise, resolve, plan, then a PNG, a dry run, a detached child or the
//! overlay in this process.

use super::config::PointConfig;
use super::output::{self, number};
use super::{detach, session, targeting};
use crate::model::display::Screens;
use crate::model::error::Result;
use crate::model::geom::Rect;
use crate::model::layout::{LayoutRequest, plan_layout};
use crate::model::placement::OtherArrow;
use crate::model::registry::Registry;
use crate::model::result::{PointResult, ResultTarget};
use crate::model::target::ResolvedTarget;
use crate::platform;
use crate::render::scene::Scene;
use crate::render::sign::SignImage;
use crate::render::text::TextEngine;

/// A planned arrow: the resolved target and the scene drawn for it.
pub struct Planned {
    pub resolved: ResolvedTarget,
    pub scene: Scene,
    /// The other arrows the placement avoided, global input units by display number.
    pub others: Vec<(usize, OtherArrow)>,
}

pub fn run(config: PointConfig) -> Result<()> {
    // A dry run or a PNG render changes nothing on screen, so it never raises an app.
    if config.raise
        && !config.dry_run
        && config.png.is_none()
        && let Some(home) = &config.home
    {
        let screens = platform::screens()?;
        let raised = targeting::raise(&home.app, home.title.as_deref(), &screens)?;
        if !raised.frontmost {
            platform::flash(raised.handle);
            output::print_error(&format!(
                "bigarrow: {} did not come to the front within 1.5 s, pointing anyway (the arrow shows once it is visible)",
                raised.app
            ));
        }
    }
    let screens = platform::screens()?;
    let resolved = targeting::resolve(&config.target, &screens)?;
    let mut engine = TextEngine::new();
    let planned = plan(&config, resolved, &screens, &mut engine, &others())?;
    if let Some(png) = &config.png {
        let mut result = result(&planned, std::process::id());
        result.image = Some(crate::render::png::write(&planned.scene, png)?);
        if config.json {
            output::json(&result)
        } else {
            output::print(&format!("bigarrow: wrote {}", png.display()))
        }
        return Ok(());
    }
    if config.dry_run {
        let mut result = result(&planned, std::process::id());
        result.dry_run = true;
        report(&result, config.json);
        return Ok(());
    }
    if config.detach {
        let pid = detach::spawn(&config, &planned)?;
        let mut result = result(&planned, pid);
        result.detached = true;
        report(&result, config.json);
        return Ok(());
    }
    session::run(config, planned, engine, false)
}

/// The other live arrows, global input units, keyed by display number.
pub fn others() -> Vec<(usize, OtherArrow)> {
    let own = std::process::id();
    Registry::new(platform::registry_dir())
        .live()
        .into_iter()
        .filter(|record| record.pid != own)
        .map(|record| {
            let [x, y, width, height] = record.sign_frame;
            let [tx, ty, tw, th] =
                record
                    .target_frame
                    .unwrap_or([record.target_x, record.target_y, 0.0, 0.0]);
            let other = OtherArrow {
                sign: Rect::new(x, y, width, height),
                target: Rect::new(tx, ty, tw, th),
            };
            (record.display, other)
        })
        .collect()
}

pub fn plan(
    config: &PointConfig,
    resolved: ResolvedTarget,
    screens: &Screens,
    engine: &mut TextEngine,
    others: &[(usize, OtherArrow)],
) -> Result<Planned> {
    let display = screens.containing(resolved.shape.anchor())?.clone();
    let sign = SignImage::new(
        engine,
        &config.sign,
        &config.look,
        display.local_bounds().width,
    );
    let local = others
        .iter()
        .filter(|(number, _)| *number == display.number())
        .map(|(_, other)| OtherArrow {
            sign: display.local_rect(&other.sign),
            target: display.local_rect(&other.target),
        })
        .collect();
    let layout = plan_layout(&LayoutRequest {
        target: resolved.shape,
        display,
        sign_size: (sign.width, sign.height),
        style: config.style,
        size: config.look.size,
        forced: config.forced,
        corners: config.look.corners,
        shape: config.shape,
        others: local,
    });
    Ok(Planned {
        resolved,
        scene: Scene::new(layout, sign, config.look.clone()),
        others: others.to_vec(),
    })
}

/// The result for this arrow, every rect in global input units.
pub fn result(planned: &Planned, pid: u32) -> PointResult {
    let layout = &planned.scene.layout;
    let display = &layout.display;
    let anchor = planned.resolved.shape.anchor();
    let rounded =
        |rect: crate::model::geom::Rect| rect.to_array().map(|value| value.round() as i64);
    let chips: Vec<[i64; 4]> = planned
        .scene
        .sign
        .chips
        .iter()
        .map(|chip| {
            rounded(display.global_rect(&chip.rect.offset(layout.sign_rect.x, layout.sign_rect.y)))
        })
        .collect();
    PointResult {
        copied: None,
        copy_buttons: (!chips.is_empty()).then_some(chips),
        detached: false,
        direction: layout.direction.name().to_owned(),
        dismissed_after: None,
        dismissed_reason: None,
        dry_run: false,
        image: None,
        ok: true,
        pid,
        sign: rounded(layout.global_sign()),
        target: ResultTarget {
            detail: planned.resolved.detail.clone(),
            display: display.number(),
            rect: planned.resolved.shape.rect().map(|rect| rect.to_array()),
            source: planned.resolved.source.clone(),
            x: anchor.x,
            y: anchor.y,
        },
    }
}

pub fn report(result: &PointResult, json: bool) {
    if json {
        output::json(result);
        return;
    }
    let target = format!(
        "{},{} on display {}",
        number(result.target.x),
        number(result.target.y),
        result.target.display
    );
    if result.dry_run {
        output::print(&format!(
            "bigarrow: would point at {target}, sign on the {}",
            result.direction
        ));
    } else if result.detached {
        output::print(&format!(
            "bigarrow: pointing at {target} in the background, pid {}; 'bigarrow clear' removes it",
            result.pid
        ));
    } else {
        let after = result.dismissed_after.map_or("?".to_owned(), |after| {
            number((after * 10.0).round() / 10.0)
        });
        let reason = result.dismissed_reason.map_or("?", |reason| reason.name());
        let copied = result
            .copied
            .map(|count| format!(", value copied {count}x"))
            .unwrap_or_default();
        output::print(&format!(
            "bigarrow: pointed at {target} for {after} s, dismissed by {reason}{copied}"
        ));
    }
}
