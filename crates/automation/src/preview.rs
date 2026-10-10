//! Document-pixel regions for headless automation previews.

use photocraft_geom::{Rect, Size};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::AutomationError;

/// Requested preview region in integer document pixels; validated before rendering.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PreviewRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl PreviewRect {
    /// Check containment and endpoints before constructing compositor geometry.
    pub fn validate(self, canvas: Size) -> Result<Rect, AutomationError> {
        let bad = |message: &str| AutomationError::BadRequest(format!("rect {message}"));
        if self.x < 0 || self.y < 0 {
            return Err(bad("origins must be nonnegative document pixels"));
        }
        if self.width == 0 || self.height == 0 {
            return Err(bad("width and height must be nonzero"));
        }
        let width = i32::try_from(self.width).map_err(|_| bad("width exceeds the document coordinate range"))?;
        let height = i32::try_from(self.height).map_err(|_| bad("height exceeds the document coordinate range"))?;
        let right = self.x.checked_add(width).ok_or_else(|| bad("x + width overflows the document coordinate range"))?;
        let bottom = self.y.checked_add(height).ok_or_else(|| bad("y + height overflows the document coordinate range"))?;
        if i64::from(right) > i64::from(canvas.width) || i64::from(bottom) > i64::from(canvas.height) {
            return Err(bad("must be fully contained in the document canvas"));
        }
        Ok(Rect::new(self.x, self.y, right, bottom))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checked_endpoints_when_the_canvas_extends_past_signed_coordinates() {
        let canvas = Size::new(u32::MAX, u32::MAX);
        for region in [
            PreviewRect { x: i32::MAX, y: 0, width: 1, height: 1 },
            PreviewRect { x: 0, y: i32::MAX, width: 1, height: 1 },
            PreviewRect { x: 0, y: 0, width: u32::MAX, height: 1 },
            PreviewRect { x: 0, y: 0, width: 1, height: u32::MAX },
        ] {
            let result = region.validate(canvas);
            assert!(matches!(result, Err(AutomationError::BadRequest(message)) if message.contains("coordinate range")));
        }
    }

    #[test]
    fn geometry_when_region_touches_both_canvas_boundaries() {
        let region = PreviewRect { x: 12, y: 6, width: 8, height: 4 };

        let rect = region.validate(Size::new(20, 10)).unwrap();

        assert_eq!(rect, Rect::new(12, 6, 20, 10));
    }
}
