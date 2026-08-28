use super::*;
use codex_protocol::openai_models::ToolMode;
use image::ImageEncoder;
use image::codecs::png::CompressionType;
use image::codecs::png::FilterType;
use image::codecs::png::PngEncoder;
use pretty_assertions::assert_eq;
use test_case::test_case;

#[derive(Clone, Copy)]
enum ResourceSource {
    Read,
    Embedded,
    EmbeddedWithStructuredContent,
}

#[test_case(ResourceSource::Read, ToolMode::Direct; "direct read")]
#[test_case(ResourceSource::Read, ToolMode::CodeMode; "code mode read")]
#[test_case(ResourceSource::Embedded, ToolMode::Direct; "direct embedded")]
#[test_case(ResourceSource::Embedded, ToolMode::CodeMode; "code mode embedded")]
#[test_case(ResourceSource::EmbeddedWithStructuredContent, ToolMode::Direct; "direct structured")]
#[test_case(ResourceSource::EmbeddedWithStructuredContent, ToolMode::CodeMode; "code mode structured")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial(mcp_test_value)]
async fn large_image_resources_reach_the_model_as_images(
    source: ResourceSource,
    mode: ToolMode,
) -> anyhow::Result<()> {
    skip_if_wine_exec!(
        Ok(()),
        "requires a Windows test_stdio_server in the Wine-exec environment"
    );
    skip_if_no_network!(Ok(()));

    // Deliberately exceed the text budget with a valid PNG. Keeping its pixel
    // dimensions small lets us verify that every byte survives unchanged.
    let mut png = Vec::new();
    PngEncoder::new_with_quality(
        &mut png,
        CompressionType::Uncompressed,
        FilterType::NoFilter,
    )
    .write_image(
        &vec![128; 128 * 128 * 3],
        /*width*/ 128,
        /*height*/ 128,
        image::ExtendedColorType::Rgb8,
    )?;
    let base64 = BASE64_STANDARD.encode(&png);
    assert!(base64.len() > 60_000);
    let data_url = format!("data:IMAGE/PNG;base64,{base64}");
    let code_mode = mode == ToolMode::CodeMode;
    let uri = "image://codex/preview.png";
    let arguments = match source {
        ResourceSource::Read => json!({"server": "rmcp", "uri": uri}),
        ResourceSource::Embedded => json!({"scenario": "embedded_resource"}),
        ResourceSource::EmbeddedWithStructuredContent => {
            json!({"scenario": "embedded_resource_with_structured_content"})
        }
    }
    .to_string();
    let call = if code_mode {
        let tool = match source {
            ResourceSource::Read => "read_mcp_resource",
            ResourceSource::Embedded | ResourceSource::EmbeddedWithStructuredContent => {
                "mcp__rmcp__image_scenario"
            }
        };
        responses::ev_custom_tool_call(
            "call-1",
            "exec",
            &format!(
                r#"
const result = await tools.{tool}({arguments});
for (const item of result.content) {{
    if (item.type === "image") image(item);
    else text(item.type === "text" ? item.text : item);
}}
if (result.structuredContent) text(result.structuredContent);
"#
            ),
        )
    } else {
        match source {
            ResourceSource::Read => {
                responses::ev_function_call("call-1", "read_mcp_resource", &arguments)
            }
            ResourceSource::Embedded | ResourceSource::EmbeddedWithStructuredContent => {
                responses::ev_function_call_with_namespace(
                    "call-1",
                    "mcp__rmcp",
                    "image_scenario",
                    &arguments,
                )
            }
        }
    };
    let server = responses::start_mock_server().await;
    mount_sse_once(
        &server,
        responses::sse(vec![call, responses::ev_completed("resp-1")]),
    )
    .await;
    let final_mock = mount_sse_once(
        &server,
        responses::sse(vec![
            responses::ev_assistant_message("msg-1", "done"),
            responses::ev_completed("resp-2"),
        ]),
    )
    .await;
    let binary = remote_aware_stdio_server_bin()?;
    let fixture = test_codex()
        .with_model("gpt-5.2")
        .with_config(move |config| {
            if code_mode {
                config
                    .features
                    .enable(Feature::CodeMode)
                    .expect("enable code mode");
            }
            insert_mcp_server(
                config,
                "rmcp",
                stdio_transport(
                    binary,
                    Some(HashMap::from([(
                        "MCP_TEST_IMAGE_RESOURCE_DATA_URL".to_string(),
                        data_url,
                    )])),
                    Vec::new(),
                ),
                TestMcpServerOptions {
                    environment_id: remote_aware_environment_id(),
                    ..Default::default()
                },
            );
        })
        .build_with_auto_env(&server)
        .await?;
    wait_for_mcp_server(&fixture.codex, "rmcp").await?;
    fixture
        .codex
        .start_or_steer_turn(read_only_user_turn(&fixture, "read the image resource"))
        .await?;
    let end = wait_for_event(&fixture.codex, |event| {
        matches!(event, EventMsg::McpToolCallEnd(_))
    })
    .await;
    let EventMsg::McpToolCallEnd(end) = end else {
        unreachable!()
    };
    let event_result = end.result.expect("resource tool should succeed");
    assert!(
        event_result
            .content
            .iter()
            .any(|item| item["type"] == "image")
    );
    wait_for_event(&fixture.codex, |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;

    let request = final_mock.single_request();
    let output = if code_mode {
        request.custom_tool_call_output("call-1")
    } else {
        request.function_call_output("call-1")
    };
    let items = output["output"].as_array().expect("multimodal output");
    let images = items
        .iter()
        .filter(|item| item["type"] == "input_image")
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(
        images,
        vec![json!({
            "type": "input_image", "image_url": format!("data:image/png;base64,{base64}"), "detail": "high",
        })]
    );
    let text = items
        .iter()
        .filter_map(|item| item["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        text.len() < 2_000,
        "base64 must not consume the text budget: {} bytes",
        text.len()
    );
    assert!(!text.contains(&base64[..64]), "raw base64 leaked into text");
    assert!(
        !text.contains("truncated"),
        "image must not truncate metadata"
    );
    match source {
        ResourceSource::Read | ResourceSource::Embedded => {
            assert!(text.contains("<image content>"))
        }
        ResourceSource::EmbeddedWithStructuredContent => assert!(text.contains("summary")),
    }
    server.verify().await;
    Ok(())
}
