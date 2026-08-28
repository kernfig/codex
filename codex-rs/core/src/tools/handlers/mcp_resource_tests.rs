use super::*;
use crate::tools::context::ToolPayload;
use codex_protocol::models::DEFAULT_IMAGE_DETAIL;
use pretty_assertions::assert_eq;
use rmcp::model::ResourceContents;
use serde_json::json;

fn resource(uri: &str, name: &str) -> Resource {
    Resource::new(uri, name)
}

fn template(uri_template: &str, name: &str) -> ResourceTemplate {
    ResourceTemplate::new(uri_template, name)
}

#[test]
fn resource_with_server_serializes_server_field() {
    let entry = ResourceWithServer::new("test".to_string(), resource("memo://id", "memo"));
    let value = serde_json::to_value(&entry).expect("serialize resource");

    assert_eq!(value["server"], json!("test"));
    assert_eq!(value["uri"], json!("memo://id"));
    assert_eq!(value["name"], json!("memo"));
}

#[test]
fn list_resources_payload_from_single_server_copies_next_cursor() {
    let mut result = ListResourcesResult::with_all_items(vec![resource("memo://id", "memo")]);
    result.next_cursor = Some("cursor-1".to_string());
    let payload = ListResourcesPayload::from_single_server("srv".to_string(), result);
    let value = serde_json::to_value(&payload).expect("serialize payload");

    assert_eq!(value["server"], json!("srv"));
    assert_eq!(value["nextCursor"], json!("cursor-1"));
    let resources = value["resources"].as_array().expect("resources array");
    assert_eq!(resources.len(), 1);
    assert_eq!(resources[0]["server"], json!("srv"));
}

#[test]
fn list_resources_payload_from_all_servers_is_sorted() {
    let mut map = HashMap::new();
    map.insert("beta".to_string(), vec![resource("memo://b-1", "b-1")]);
    map.insert(
        "alpha".to_string(),
        vec![resource("memo://a-1", "a-1"), resource("memo://a-2", "a-2")],
    );

    let payload = ListResourcesPayload::from_all_servers(map);
    let value = serde_json::to_value(&payload).expect("serialize payload");
    let uris: Vec<String> = value["resources"]
        .as_array()
        .expect("resources array")
        .iter()
        .map(|entry| entry["uri"].as_str().unwrap().to_string())
        .collect();

    assert_eq!(
        uris,
        vec![
            "memo://a-1".to_string(),
            "memo://a-2".to_string(),
            "memo://b-1".to_string()
        ]
    );
}

#[test]
fn serialized_resource_output_marks_success() {
    let output = serialize_function_output(json!({}), TruncationPolicy::Bytes(1_024)).unwrap();

    assert_eq!(
        output.0,
        CallToolResult {
            content: vec![json!({"type": "text", "text": "{}"})],
            structured_content: None,
            is_error: Some(false),
            meta: None,
        }
    );
}

#[test]
fn parse_arguments_handles_empty_and_json() {
    assert!(
        parse_arguments(" \n\t").unwrap().is_none(),
        "expected None for empty arguments"
    );

    assert!(
        parse_arguments("null").unwrap().is_none(),
        "expected None for null arguments"
    );

    let value = parse_arguments(r#"{"server":"figma"}"#)
        .expect("parse json")
        .expect("value present");
    assert_eq!(value["server"], json!("figma"));
}

#[test]
fn list_resource_args_normalizes_server_and_cursor() {
    let args: ListResourceArgs = serde_json::from_value(json!({
        "server": "  hosted  ",
        "cursor": "  next-page  "
    }))
    .expect("parse resource-list arguments");

    assert_eq!(
        args.normalized(),
        ListResourceArgs {
            server: Some("hosted".to_string()),
            cursor: Some("next-page".to_string()),
        }
    );
}

#[test]
fn template_with_server_serializes_server_field() {
    let entry = ResourceWithServer::new("srv".to_string(), template("memo://{id}", "memo"));
    let value = serde_json::to_value(&entry).expect("serialize template");

    assert_eq!(
        value,
        json!({
            "server": "srv",
            "uriTemplate": "memo://{id}",
            "name": "memo"
        })
    );
}

#[test]
fn list_resource_templates_payload_from_all_servers_is_sorted() {
    let mut templates_by_server = HashMap::new();
    templates_by_server.insert(
        "beta".to_string(),
        vec![template("memo://beta/{id}", "beta")],
    );
    templates_by_server.insert(
        "alpha".to_string(),
        vec![template("memo://alpha/{id}", "alpha")],
    );

    let payload = ListResourceTemplatesPayload::from_all_servers(templates_by_server);

    assert_eq!(
        serde_json::to_value(payload).expect("serialize resource templates"),
        json!({
            "resourceTemplates": [
                {"server": "alpha", "uriTemplate": "memo://alpha/{id}", "name": "alpha"},
                {"server": "beta", "uriTemplate": "memo://beta/{id}", "name": "beta"}
            ]
        })
    );
}

#[test]
fn serialize_function_output_preserves_small_payload() {
    let payload = json!({"server": "hosted", "resources": []});
    let expected = serde_json::to_string(&payload).expect("serialize payload");

    let output = serialize_function_output(payload, TruncationPolicy::Bytes(1_024))
        .expect("serialize function output");
    let tool_payload = ToolPayload::Function {
        arguments: "{}".to_string(),
    };
    assert_eq!(
        output.code_mode_result(&tool_payload),
        Value::String(expected.clone())
    );
    let ResponseInputItem::FunctionCallOutput {
        output: response, ..
    } = output.to_response_item("list", &tool_payload)
    else {
        panic!("expected function output");
    };
    assert_eq!(
        response.body,
        FunctionCallOutputBody::Text(expected.clone())
    );

    assert_eq!(output.log_output(), expected);
}

#[test]
fn serialize_function_output_caps_read_resource_payload() {
    let truncation_policy = TruncationPolicy::Bytes(8_000);
    let payload = ReadResourcePayload {
        server: "hosted".to_string(),
        uri: "skill://large/SKILL.md".to_string(),
        result: ReadResourceResult::new(vec![ResourceContents::TextResourceContents {
            uri: "skill://large/SKILL.md".to_string(),
            mime_type: Some("text/markdown".to_string()),
            text: "x".repeat(16_000),
            meta: None,
        }]),
    };
    let serialized = serde_json::to_string(&payload).expect("serialize payload");
    let expected = truncate_text(&serialized, truncation_policy * 1.2);

    let output = serialize_read_resource_output(payload, truncation_policy)
        .expect("serialize bounded function output")
        .log_output();

    assert_ne!(output, serialized);
    assert_eq!(output, expected);
}

#[test]
fn serialize_read_resource_output_emits_image_blobs_as_image_items() {
    let image_blob = "iVBORw0KGgo=".repeat(10_000);
    let binary_blob = "AAEC";
    let payload = ReadResourcePayload {
        server: "hosted".to_string(),
        uri: "asset://collection".to_string(),
        result: ReadResourceResult::new(vec![
            ResourceContents::BlobResourceContents {
                uri: "asset://preview.png".to_string(),
                mime_type: Some("IMAGE/PNG".to_string()),
                blob: image_blob.clone(),
                meta: None,
            },
            ResourceContents::BlobResourceContents {
                uri: "asset://archive.bin".to_string(),
                mime_type: Some("application/octet-stream".to_string()),
                blob: binary_blob.to_string(),
                meta: None,
            },
        ]),
    };
    let output = serialize_read_resource_output(payload, TruncationPolicy::Bytes(1_024))
        .expect("serialize image resource output");

    let text = output.log_output();
    let expected_metadata = json!({
        "server": "hosted",
        "uri": "asset://collection",
        "contents": [
            {
                "uri": "asset://preview.png",
                "mimeType": "IMAGE/PNG",
                "blob": "<image content>",
            },
            {
                "uri": "asset://archive.bin",
                "mimeType": "application/octet-stream",
                "blob": binary_blob,
            },
        ],
        "resultType": "complete",
    });
    assert_eq!(
        serde_json::from_str::<Value>(&text).expect("parse resource metadata"),
        expected_metadata
    );
    let response = output.to_response_item(
        "read-image",
        &ToolPayload::Function {
            arguments: "{}".to_string(),
        },
    );
    let ResponseInputItem::FunctionCallOutput {
        output: response_output,
        ..
    } = &response
    else {
        panic!("expected function output");
    };
    assert_eq!(
        &response_output.content_items().unwrap()[1..],
        &[FunctionCallOutputContentItem::InputImage {
            image_url: format!("data:image/png;base64,{image_blob}"),
            detail: Some(DEFAULT_IMAGE_DETAIL),
        }]
    );
    assert_eq!(
        output.code_mode_result(&ToolPayload::Function {
            arguments: "{}".to_string(),
        }),
        serde_json::to_value(&output.0).unwrap()
    );
    let estimated_tokens = crate::context_manager::estimate_item_token_count(&response.into());
    assert!(
        estimated_tokens < 3_000,
        "image base64 must not be counted as text: {estimated_tokens}"
    );

    let event_result = output.0;
    assert_eq!(event_result.content.len(), 2);
    assert_eq!(
        serde_json::from_str::<Value>(
            event_result.content[0]["text"]
                .as_str()
                .expect("event resource metadata text"),
        )
        .expect("parse event resource metadata"),
        expected_metadata
    );
    assert_eq!(
        event_result.content[1],
        json!({
            "type": "image",
            "data": image_blob,
            "mimeType": "image/png",
        })
    );
}
