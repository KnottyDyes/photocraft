use base64::Engine as _;
use photocraft_automation::{AutomationError, Headless, PreviewRect, files, rpc};
use photocraft_doc::{ColorMode, Document, Layer, SampleType};
use photocraft_geom::{Rect, Size};
use serde_json::json;
use std::sync::Mutex;

fn sparse(width: u32, height: u32) -> Headless {
    let mut headless = Headless::new();
    headless.session.add_document(Document::new("sparse", Size::new(width, height), ColorMode::Rgb, SampleType::U8), None);
    headless
}

#[test]
fn small_region_when_canvas_exceeds_source_budget() {
    let headless = sparse(8193, 8192);
    assert!(matches!(headless.render_png(None, 16), Err(AutomationError::BadRequest(message)) if message.contains("source exceeds")));
    let rect = PreviewRect { x: 8177, y: 8184, width: 16, height: 8 };

    let png = headless.render_png_region(None, rect, 0).unwrap();

    let image = photocraft_codecs::decode(&png).unwrap();
    assert_eq!(image.dimensions(), (16, 8));
    assert!(image.to_rgba8().iter().all(|byte| *byte == 0));
}

#[test]
fn region_rejected_when_it_exceeds_existing_preview_budgets() {
    let headless = sparse(8193, 8192);
    for (rect, max_side, expected) in [
        (PreviewRect { x: 0, y: 0, width: 8193, height: 8192 }, 16, "source exceeds"),
        (PreviewRect { x: 0, y: 0, width: 2049, height: 1 }, 0, "side exceeds"),
        (PreviewRect { x: 0, y: 0, width: 1, height: 1 }, 2049, "side exceeds"),
    ] {
        let result = headless.render_png_region(None, rect, max_side);
        assert!(matches!(result, Err(AutomationError::BadRequest(message)) if message.contains(expected)));
    }
}

#[test]
fn region_rejected_when_geometry_is_empty_negative_outside_or_overflowing() {
    let headless = sparse(20, 10);
    for rect in [
        PreviewRect { x: 0, y: 0, width: 0, height: 1 },
        PreviewRect { x: 0, y: 0, width: 1, height: 0 },
        PreviewRect { x: -1, y: 0, width: 1, height: 1 },
        PreviewRect { x: 0, y: -1, width: 1, height: 1 },
        PreviewRect { x: 20, y: 0, width: 1, height: 1 },
        PreviewRect { x: 19, y: 9, width: 2, height: 2 },
        PreviewRect { x: i32::MAX, y: 0, width: 1, height: 1 },
        PreviewRect { x: 0, y: i32::MAX, width: 1, height: 1 },
        PreviewRect { x: 0, y: 0, width: u32::MAX, height: 1 },
    ] {
        let result = headless.render_png_region(None, rect, 16);
        assert!(matches!(result, Err(AutomationError::BadRequest(message)) if message.starts_with("rect ")));
    }
}

#[test]
fn region_pixels_when_origin_is_offset_and_output_is_scaled() {
    for depth in [SampleType::U8, SampleType::U16, SampleType::F32] {
        let mut doc = Document::new("offset", Size::new(20, 10), ColorMode::Rgb, depth);
        let mut layer = Layer::raster("pattern", doc.pixel_format());
        let surface = layer.surface_mut().unwrap();
        surface.fill_rect(Rect::new(0, 0, 20, 10), &[0.0, 0.0, 1.0, 1.0]);
        surface.fill_rect(Rect::new(12, 6, 16, 10), &[1.0, 0.0, 0.0, 1.0]);
        surface.fill_rect(Rect::new(16, 6, 20, 10), &[0.0, 1.0, 0.0, 1.0]);
        doc.layers.push(layer);
        let rect = PreviewRect { x: 12, y: 6, width: 8, height: 4 };

        let png = files::render_png_region(&doc, rect, 4).unwrap();

        let image = photocraft_codecs::decode(&png).unwrap();
        assert_eq!(image.dimensions(), (4, 2));
        assert_eq!(image.to_rgba8(), [255, 0, 0, 255, 255, 0, 0, 255, 0, 255, 0, 255, 0, 255, 0, 255].repeat(2));
    }
}

#[test]
fn region_dimensions_when_full_size_rounding_or_no_upscale_is_requested() {
    let headless = sparse(20, 10);
    let rect = PreviewRect { x: 13, y: 7, width: 7, height: 3 };
    for (max_side, dimensions) in [(0, (7, 3)), (100, (7, 3)), (4, (4, 2)), (1, (1, 1))] {
        let png = headless.render_png_region(None, rect, max_side).unwrap();
        assert_eq!(photocraft_codecs::decode(&png).unwrap().dimensions(), dimensions);
    }
}

#[test]
fn trusted_local_region_when_requested_side_exceeds_automation_ceiling() {
    let mut headless = Headless::trusted_local();
    headless.session.add_document(Document::new("local", Size::new(2049, 1), ColorMode::Rgb, SampleType::U8), None);

    let png = headless.render_png_region(None, PreviewRect { x: 0, y: 0, width: 2049, height: 1 }, 0).unwrap();

    assert_eq!(photocraft_codecs::decode(&png).unwrap().dimensions(), (2049, 1));
}

#[test]
fn png_budget_when_region_contains_incompressible_pixels() {
    let mut doc = Document::new("noise", Size::new(1500, 1000), ColorMode::Rgb, SampleType::U8);
    let mut layer = Layer::raster("noise", doc.pixel_format());
    let mut pixels = vec![0; 1500 * 1000 * 4];
    let mut state = 0x6d2b_79f5_u32;
    for byte in &mut pixels {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        *byte = state.to_le_bytes()[0];
    }
    layer.surface_mut().unwrap().write_interleaved(Rect::new(0, 0, 1500, 1000), &pixels);
    doc.layers.push(layer);
    let mut automation = Headless::new();
    automation.session.add_document(doc.clone(), None);
    let mut trusted = Headless::trusted_local();
    trusted.session.add_document(doc, None);
    let region = PreviewRect { x: 0, y: 0, width: 1500, height: 1000 };

    let result = automation.render_png_region(None, region, 0);

    assert!(matches!(result, Err(AutomationError::BadRequest(message)) if message.contains("automation PNG exceeds")));
    assert!(trusted.render_png_region(None, region, 0).unwrap().len() > photocraft_automation::budgets::MAX_PNG_BYTES);
}

#[test]
fn no_region_behavior_when_errors_indices_or_null_rect_are_used() {
    let mut headless = Headless::new();
    assert_eq!(headless.render_png(None, 16).unwrap_err().to_string(), "no document open");
    assert_eq!(headless.render_png(Some(0), 16).unwrap_err().to_string(), "no document at index 0");
    headless.session.add_document(Document::new("first", Size::new(12, 6), ColorMode::Rgb, SampleType::U8), None);
    headless.session.add_document(Document::new("second", Size::new(6, 12), ColorMode::Rgb, SampleType::U8), None);
    let expected = files::render_png(&headless.session.documents()[0].doc, 6).unwrap();

    let reply = headless.handle("doc.render", json!({"index": 0, "maxSide": 6, "rect": null})).unwrap();

    assert_eq!(base64::engine::general_purpose::STANDARD.decode(reply["base64"].as_str().unwrap()).unwrap(), expected);
    assert_eq!(headless.session.active_index(), Some(1));
    assert_eq!(headless.handle("doc.render", json!({"maxSide": -1})).unwrap_err().to_string(), "maxSide must be an unsigned 32-bit integer");
    assert_eq!(headless.render_png(None, 2049).unwrap_err().to_string(), "automation preview side exceeds 2048 pixels");
}

#[test]
fn json_lines_region_when_fields_are_malformed_and_session_recovers() {
    let headless = Mutex::new(sparse(20, 10));
    for rect in [
        json!({}),
        json!({"x": 0, "y": 0, "width": 1}),
        json!({"x": 0.5, "y": 0, "width": 1, "height": 1}),
        json!({"x": 0, "y": "0", "width": 1, "height": 1}),
        json!({"x": 0, "y": 0, "width": -1, "height": 1}),
        json!({"x": 0, "y": 0, "width": 1, "height": 1, "extra": 1}),
        json!({"x": 2147483648_u64, "y": 0, "width": 1, "height": 1}),
        json!({"x": 0, "y": 0, "width": 4294967296_u64, "height": 1}),
        json!([]),
    ] {
        let input = format!(
            "{}\n{}\n",
            json!({"id": 1, "method": "doc.render", "params": {"rect": rect}}),
            json!({"id": 2, "method": "doc.render", "params": {"rect": {"x": 16, "y": 8, "width": 4, "height": 2}, "maxSide": 0}})
        );
        let mut output = Vec::new();

        rpc::serve_lines(&headless, input.as_bytes(), &mut output).unwrap();

        let replies: Vec<serde_json::Value> = String::from_utf8(output).unwrap().lines().map(|line| serde_json::from_str(line).unwrap()).collect();
        assert_eq!(replies[0]["ok"], false);
        assert!(replies[0]["error"].as_str().unwrap().contains("invalid rect"));
        assert_eq!(replies[1]["ok"], true);
        let png = base64::engine::general_purpose::STANDARD.decode(replies[1]["result"]["base64"].as_str().unwrap()).unwrap();
        assert_eq!(photocraft_codecs::decode(&png).unwrap().dimensions(), (4, 2));
    }
}
