use super::*;
use photocraft_automation::{Backend, Headless};
use photocraft_doc::{ColorMode, Document, Layer, SampleType};
use photocraft_geom::{Rect, Size};
use std::sync::{Arc, Mutex};

#[tokio::test(flavor = "multi_thread")]
async fn headless_regions_when_canvas_exceeds_budget_and_index_is_inactive() {
    let mut headless = Headless::new();
    let mut doc = Document::new("large", Size::new(8193, 8192), ColorMode::Rgb, SampleType::U8);
    let mut layer = Layer::raster("offset", doc.pixel_format());
    layer.surface_mut().unwrap().fill_rect(Rect::new(8185, 8188, 8193, 8192), &[1.0, 0.0, 0.0, 1.0]);
    doc.layers.push(layer);
    headless.session.add_document(doc, None);
    headless.session.add_document(Document::new("active", Size::new(1, 1), ColorMode::Rgb, SampleType::U8), None);
    let client = connect(PhotocraftMcp::with_backend(Backend::Headless(Arc::new(Mutex::new(headless))))).await;
    let rejected = call(&client, "doc_render_preview", json!({"index": 0, "max_side": 4})).await;
    assert_eq!(rejected.is_error, Some(true));
    assert!(text(&rejected).contains("source exceeds"));

    for tool in ["doc_render_preview", "render_preview"] {
        let reply = call(&client, tool, json!({"index": 0, "rect": {"x": 8185, "y": 8188, "width": 8, "height": 4}, "max_side": 4})).await;
        assert_ne!(reply.is_error, Some(true), "{}", text(&reply));
        let image = reply.content.iter().find_map(|content| content.as_image()).unwrap();
        let png = base64::engine::general_purpose::STANDARD.decode(&image.data).unwrap();
        let decoded = photocraft_codecs::decode(&png).unwrap();
        assert_eq!(decoded.dimensions(), (4, 2));
        assert_eq!(decoded.to_rgba8(), [255, 0, 0, 255].repeat(8));
    }
    assert_eq!(json_of(&call(&client, "session_list", json!({})).await)["active"], 1);
    let tools = client.list_all_tools().await.unwrap();
    let schema = &tools.iter().find(|tool| tool.name == "doc_render_preview").unwrap().input_schema;
    assert!(schema["properties"].get("rect").is_some());
    client.cancel().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn headless_regions_when_geometry_or_fields_are_invalid() {
    let client = connect(PhotocraftMcp::headless()).await;
    json_of(&call(&client, "doc_new", json!({"width": 20, "height": 10, "background": "transparent"})).await);
    for rect in [
        json!({"x": 0, "y": 0, "width": 0, "height": 1}),
        json!({"x": -1, "y": 0, "width": 1, "height": 1}),
        json!({"x": 20, "y": 0, "width": 1, "height": 1}),
        json!({"x": 2147483647, "y": 0, "width": 1, "height": 1}),
        json!({"x": 0, "y": 0, "width": 1}),
        json!({"x": 0.5, "y": 0, "width": 1, "height": 1}),
        json!({"x": 0, "y": 0, "width": 1, "height": 1, "extra": true}),
    ] {
        let args = json!({"rect": rect}).as_object().unwrap().clone();
        match client.call_tool(CallToolRequestParams::new("doc_render_preview").with_arguments(args)).await {
            Err(_) => {}
            Ok(reply) => assert_eq!(reply.is_error, Some(true), "{}", text(&reply)),
        }
    }
    let reply = call(&client, "doc_render_preview", json!({"rect": {"x": 16, "y": 8, "width": 4, "height": 2}, "max_side": 0})).await;
    assert_ne!(reply.is_error, Some(true), "{}", text(&reply));
    client.cancel().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn bridge_regions_when_rect_is_requested_never_capture_screenshot() {
    let (addr, app) = fake_app().await;
    let client = connect(PhotocraftMcp::bridge(&addr, CONTROL_TOKEN).unwrap()).await;
    for tool in ["doc_render_preview", "render_preview"] {
        for index in [None, Some(0)] {
            let mut args = json!({"rect": {"x": 0, "y": 0, "width": 1, "height": 1}});
            if let Some(index) = index {
                args["index"] = json!(index);
            }
            let reply = call(&client, tool, args).await;
            assert_eq!(reply.is_error, Some(true));
            assert!(text(&reply).contains(if index.is_some() { "`index`" } else { "`rect`" }));
        }
    }
    let reply = call(&client, "doc_render_preview", json!({"max_side": 20})).await;
    assert_ne!(reply.is_error, Some(true));
    client.cancel().await.unwrap();
    let seen = tokio::time::timeout(std::time::Duration::from_secs(5), app).await.unwrap().unwrap();
    assert_eq!(seen.iter().filter(|request| request["method"] == "ui.screenshot").count(), 1);
}
