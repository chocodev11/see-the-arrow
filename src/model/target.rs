use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::error::{Error, Result};
use super::geom::{Point, Rect};

/// What the arrow points at, in global input units.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum TargetShape {
    Point(Point),
    Rect(Rect),
}

impl TargetShape {
    pub fn anchor(&self) -> Point {
        match self {
            Self::Point(point) => *point,
            Self::Rect(rect) => rect.center(),
        }
    }

    pub fn rect(&self) -> Option<Rect> {
        match self {
            Self::Point(_) => None,
            Self::Rect(rect) => Some(*rect),
        }
    }
}

/// A resolved target plus where it came from, for `--json` output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResolvedTarget {
    pub shape: TargetShape,
    pub source: String,
    pub detail: BTreeMap<String, String>,
}

impl ResolvedTarget {
    pub fn new(shape: TargetShape, source: &str) -> Self {
        Self {
            shape,
            source: source.to_owned(),
            detail: BTreeMap::new(),
        }
    }
}

/// `--at 760,500`.
pub fn parse_point(raw: &str) -> Result<Point> {
    let values = numbers(raw, 2, "760,500")?;
    Ok(Point::new(values[0], values[1]))
}

/// `--rect 400,300,200,80`.
pub fn parse_rect(raw: &str) -> Result<Rect> {
    let values = numbers(raw, 4, "400,300,200,80")?;
    if values[2] <= 0.0 || values[3] <= 0.0 {
        return Err(Error::bad_input(format!(
            "rect '{raw}' needs a positive width and height"
        )));
    }
    Ok(Rect::new(values[0], values[1], values[2], values[3]))
}

fn numbers(raw: &str, count: usize, example: &str) -> Result<Vec<f64>> {
    let values: Vec<f64> = raw
        .split(',')
        .filter_map(|part| part.trim().parse().ok())
        .collect();
    if raw.split(',').count() != count
        || values.len() != count
        || !values.iter().all(|value| value.is_finite())
    {
        return Err(Error::bad_input(format!(
            "'{raw}' is not {count} comma-separated numbers, example: {example}"
        )));
    }
    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn points_and_rects_parse() {
        assert_eq!(parse_point("760,500").unwrap(), Point::new(760.0, 500.0));
        assert_eq!(parse_point(" -12.5 , 3 ").unwrap(), Point::new(-12.5, 3.0));
        assert_eq!(
            parse_rect("400,300,200,80").unwrap(),
            Rect::new(400.0, 300.0, 200.0, 80.0)
        );
    }

    #[test]
    fn malformed_numbers_are_bad_input() {
        for raw in ["760", "760,500,1", "a,b", "760;500", "", "nan,1"] {
            let error = parse_point(raw).unwrap_err();
            assert_eq!(
                error,
                Error::bad_input(format!(
                    "'{raw}' is not 2 comma-separated numbers, example: 760,500"
                ))
            );
        }
        assert!(parse_rect("1,2,0,5").is_err());
        assert!(parse_rect("1,2,3").is_err());
    }
}
