use crate::mcp::CallToolResult;
use serde_json::Value;
use serde_json::json;

/// Moves an image resource's base64 into an MCP image block, leaving its
/// provenance in place without duplicating the image bytes in text context.
pub fn extract_resource_image(resource: &mut Value) -> Option<Value> {
    let mime_type = resource
        .get("mimeType")?
        .as_str()?
        .split(';')
        .next()?
        .trim();
    if !mime_type
        .get(..6)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("image/"))
        || mime_type.len() == 6
        || !resource.get("blob")?.is_string()
    {
        return None;
    }
    let mime_type = mime_type.to_ascii_lowercase();
    let data = std::mem::replace(resource.get_mut("blob")?, json!("<image content>"));
    let mut image = json!({"type": "image", "mimeType": mime_type, "data": data});
    if let Some(detail) = resource
        .get("_meta")
        .and_then(|meta| meta.get("codex/imageDetail"))
    {
        image["_meta"] = json!({"codex/imageDetail": detail});
    }
    Some(image)
}

impl CallToolResult {
    /// Promotes embedded image resources before modality filtering and before
    /// the result is exposed to direct callers or Code Mode's `image()` helper.
    pub fn normalize_resource_images(&mut self) {
        self.content = std::mem::take(&mut self.content)
            .into_iter()
            .flat_map(|mut block| {
                let image = if block.get("type").and_then(Value::as_str) == Some("resource") {
                    block.get_mut("resource").and_then(extract_resource_image)
                } else {
                    None
                };
                if image.is_some() {
                    block = json!({"type": "text", "text": block.to_string()});
                }
                std::iter::once(block).chain(image)
            })
            .collect();
    }
}

#[cfg(test)]
#[path = "mcp_resource_images_tests.rs"]
mod tests;
