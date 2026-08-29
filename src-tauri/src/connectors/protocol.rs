use super::*;
use serde_json::{json, Value};

pub async fn discover(endpoint: &str, slug: &str) -> Result<Vec<String>, String> {
    let client = http_client(CONNECT_TIMEOUT_SECS)?;
    let health = client
        .get(format!("{endpoint}/health"))
        .send()
        .await
        .map_err(|error| format!("CONNECTOR_PROBE_FAILED: {error}"))?;
    if !health.status().is_success() {
        return Err(format!(
            "CONNECTOR_PROBE_FAILED: health {}",
            health.status()
        ));
    }
    let listed = client
        .get(format!("{endpoint}/v1/connectors/{slug}/tools"))
        .send()
        .await
        .map_err(|error| format!("CONNECTOR_PROBE_FAILED: {error}"))?;
    if !listed.status().is_success() {
        return Err(format!("CONNECTOR_PROBE_FAILED: tools {}", listed.status()));
    }
    let payload: Value = listed
        .json()
        .await
        .map_err(|error| format!("CONNECTOR_PROBE_FAILED: {error}"))?;
    let tools = payload
        .get("tools")
        .and_then(Value::as_array)
        .ok_or_else(|| "CONNECTOR_PROBE_FAILED: tools array missing".to_string())?;
    let mut names = Vec::new();
    for tool in tools {
        let name = tool
            .pointer("/function/name")
            .or_else(|| tool.get("name"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if name.is_empty() {
            continue;
        }
        if !tool_allowed_for_slug(slug, name) {
            return Err(format!(
                "CONNECTOR_PROBE_FAILED: tool `{name}` is not allowed for `{slug}`"
            ));
        }
        names.push(name.to_string());
    }
    if names.is_empty() {
        return Err("CONNECTOR_PROBE_FAILED: connector advertised no tools".into());
    }
    Ok(names)
}

pub async fn invoke(slug: &str, name: &str, arguments: &Value) -> String {
    let endpoint = match configured_open_connector_url() {
        Ok(Some(url)) => url,
        Ok(None) => {
            return "CONNECTOR_RUNTIME_MISSING: Open Connector is not configured".into();
        }
        Err(error) => return error,
    };
    let client = match http_client(INVOKE_TIMEOUT_SECS) {
        Ok(client) => client,
        Err(error) => return error,
    };
    match client
        .post(format!("{endpoint}/v1/connectors/{slug}/invoke"))
        .json(&json!({ "name": name, "arguments": arguments }))
        .send()
        .await
    {
        Ok(response) => match response.json::<Value>().await {
            Ok(payload) => {
                if payload.get("ok").and_then(Value::as_bool) == Some(true) {
                    bound_output(
                        payload
                            .get("output")
                            .map(|value| match value {
                                Value::String(text) => text.clone(),
                                other => other.to_string(),
                            })
                            .unwrap_or_default(),
                    )
                } else {
                    payload
                        .get("error")
                        .and_then(Value::as_str)
                        .unwrap_or("connector invoke failed")
                        .to_string()
                }
            }
            Err(error) => format!("CONNECTOR_INVOKE_FAILED: {error}"),
        },
        Err(error) => format!("CONNECTOR_INVOKE_FAILED: {error}"),
    }
}
