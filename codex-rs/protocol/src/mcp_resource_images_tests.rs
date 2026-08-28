use super::*;
use crate::models::FunctionCallOutputBody;
use crate::models::FunctionCallOutputContentItem;
use crate::models::FunctionCallOutputPayload;
use crate::models::ImageDetail;
use pretty_assertions::assert_eq;

#[test]
fn image_resources_preserve_provenance_and_image_detail() {
    for mime_type in ["image/png", "IMAGE/PNG", "image/PNG; charset=binary"] {
        let mut resource = json!({
            "uri": "asset://preview", "mimeType": mime_type, "blob": "YWJj",
            "_meta": {"codex/imageDetail": "original", "private": "metadata"},
        });
        assert_eq!(
            extract_resource_image(&mut resource),
            Some(json!({
                "type": "image", "mimeType": "image/png", "data": "YWJj",
                "_meta": {"codex/imageDetail": "original"},
            }))
        );
        assert_eq!(
            resource,
            json!({
                "uri": "asset://preview", "mimeType": mime_type, "blob": "<image content>",
                "_meta": {"codex/imageDetail": "original", "private": "metadata"},
            })
        );
    }
}

#[test]
fn non_image_resources_are_unchanged() {
    for mut resource in [
        json!({"uri": "asset://binary", "mimeType": "application/octet-stream", "blob": "YWJj"}),
        json!({"uri": "asset://unknown", "blob": "YWJj"}),
        json!({"uri": "asset://vector", "mimeType": "image/svg+xml", "text": "<svg/>"}),
        json!({"uri": "asset://invalid", "mimeType": "image/png", "blob": 42}),
    ] {
        let original = resource.clone();
        assert_eq!(extract_resource_image(&mut resource), None);
        assert_eq!(resource, original);
    }
}

#[test]
fn embedded_images_keep_order_and_survive_structured_content() {
    let mut result = CallToolResult {
        content: vec![
            json!({"type": "text", "text": "caption"}),
            json!({"type": "resource", "resource": {
                "uri": "asset://preview", "mimeType": "IMAGE/PNG", "blob": "YWJj",
                "_meta": {"codex/imageDetail": "original"},
            }}),
            json!({"type": "resource", "resource": {
                "uri": "asset://binary", "mimeType": "application/octet-stream", "blob": "AAEC",
            }}),
            json!({"type": "image", "mimeType": "image/png", "data": "ZGVm"}),
        ],
        structured_content: Some(json!({"summary": "two images"})),
        is_error: Some(false),
        meta: None,
    };
    let mut expected = result.clone();
    expected.content[1]["resource"]["blob"] = json!("<image content>");
    expected.content[1] = json!({"type": "text", "text": expected.content[1].to_string()});
    expected.content.insert(
        /*index*/ 2,
        json!({
            "type": "image", "mimeType": "image/png", "data": "YWJj",
            "_meta": {"codex/imageDetail": "original"},
        }),
    );
    result.normalize_resource_images();
    assert_eq!(result, expected);
    result.normalize_resource_images();
    assert_eq!(result, expected);
    assert_eq!(
        result.as_function_call_output_payload(),
        FunctionCallOutputPayload {
            body: FunctionCallOutputBody::ContentItems(vec![
                FunctionCallOutputContentItem::InputText {
                    text: json!({"summary": "two images"}).to_string(),
                },
                FunctionCallOutputContentItem::InputImage {
                    image_url: "data:image/png;base64,YWJj".to_string(),
                    detail: Some(ImageDetail::Original),
                },
                FunctionCallOutputContentItem::InputImage {
                    image_url: "data:image/png;base64,ZGVm".to_string(),
                    detail: Some(ImageDetail::High),
                },
            ]),
            success: Some(true),
        }
    );
}
