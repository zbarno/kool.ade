use super::super::ServerConfig;
use crate::computer_use::mcp::{
    ComputerUseMcpRequestV1 as Request, ComputerUseObservationKindV1 as Observation,
};
use serde_json::{Value, json};
use std::path::Path;

const NAMES: [&str; 8] = [
    "koolade_computer_use_capabilities",
    "koolade_computer_use_request_session",
    "koolade_computer_use_start_session",
    "koolade_computer_use_observe",
    "koolade_computer_use_action",
    "koolade_computer_use_pause",
    "koolade_computer_use_resume",
    "koolade_computer_use_release",
];

pub(super) fn is_tool(name: &str) -> bool {
    NAMES.contains(&name)
}

pub(super) fn list() -> Vec<Value> {
    [
        (NAMES[0], "Discover capabilities approved for this task.", json!({"type":"object","properties":{},"additionalProperties":false})),
        (NAMES[1], "Request an application-mediated Computer Use session for a specific purpose.", json!({"type":"object","properties":{"purpose":{"type":"string","maxLength":512}},"required":["purpose"],"additionalProperties":false})),
        (NAMES[2], "Start the approved session identified by the application.", json!({"type":"object","properties":{"session_id":{"type":"string"}},"required":["session_id"],"additionalProperties":false})),
        (NAMES[3], "Observe an approved screenshot, window list, accessibility tree, or state.", json!({"type":"object","properties":{"session_id":{"type":"string"},"observation":{"type":"string","enum":["screenshot","windows","accessibility","state"]}},"required":["session_id","observation"],"additionalProperties":false})),
        (NAMES[4], "Perform one policy-checked action in the approved session.", json!({"type":"object","properties":{"session_id":{"type":"string"},"payload":{"type":"object"},"timeout_ms":{"type":"integer","minimum":1,"maximum":60000}},"required":["session_id","payload"],"additionalProperties":false})),
        (NAMES[5], "Pause the approved Computer Use session.", json!({"type":"object","properties":{"session_id":{"type":"string"}},"required":["session_id"],"additionalProperties":false})),
        (NAMES[6], "Resume the approved Computer Use session after policy revalidation.", json!({"type":"object","properties":{"session_id":{"type":"string"}},"required":["session_id"],"additionalProperties":false})),
        (NAMES[7], "Release the approved Computer Use session.", json!({"type":"object","properties":{"session_id":{"type":"string"}},"required":["session_id"],"additionalProperties":false})),
    ].into_iter().map(|(name, description, input_schema)| json!({"name":name,"description":description,"inputSchema":input_schema})).collect()
}

pub(super) fn call(
    config_path: &Path,
    config: &ServerConfig,
    name: &str,
    arguments: &Value,
) -> super::CallResponse {
    let parsed = parse(name, arguments);
    let response = parsed
        .map_err(|error| error.to_owned())
        .and_then(|request| {
            let config_binding = config.computer_use.as_ref().ok_or_else(|| "Computer Use broker is unavailable".to_owned())?;
            let binding = crate::harness::execution_security::computer_use_bridge::ComputerUseBridgeBindingV1 {
                socket_path: config_binding.socket_path.clone(),
                bridge_session_id: config_binding.bridge_session_id.clone(),
                capability_id: config_binding.capability_id.clone(),
                task_id: config_binding.task_id.clone(),
            };
            super::super::super::computer_use_bridge::call(config_path, &binding, request)
                .map_err(|_| "Computer Use request could not be authorized by the application".to_owned())
        });
    match response {
        Ok(mut response) => {
            let image = response.observation.as_mut().and_then(|observation| {
                if observation.screenshot_png_base64.is_empty() {
                    None
                } else {
                    Some(std::mem::take(&mut observation.screenshot_png_base64))
                }
            });
            let mut content = vec![
                json!({"type":"text","text":serde_json::to_string(&response).unwrap_or_else(|_| "Computer Use response could not be encoded.".into())}),
            ];
            if let Some(data) = image {
                content.push(json!({"type":"image","data":data,"mimeType":"image/png"}));
            }
            super::CallResponse {
                response: json!({"content":content,"isError":matches!(response.status, crate::computer_use::mcp::ComputerUseMcpStatusV1::Denied | crate::computer_use::mcp::ComputerUseMcpStatusV1::Failed)}),
                after_delivery: None,
            }
        }
        Err(message) => super::CallResponse {
            response: json!({"content":[{"type":"text","text":message}],"isError":true}),
            after_delivery: None,
        },
    }
}

fn parse(name: &str, args: &Value) -> Result<Request, &'static str> {
    if !args.is_object() {
        return Err("Computer Use tool arguments must be an object");
    }
    let session = || {
        args["session_id"]
            .as_str()
            .map(str::to_owned)
            .ok_or("A session_id is required")
    };
    match name {
        "koolade_computer_use_capabilities" => {
            check_keys(args, &[])?;
            Ok(Request::Capabilities)
        }
        "koolade_computer_use_request_session" => {
            check_keys(args, &["purpose"])?;
            Ok(Request::RequestSession {
                purpose: args["purpose"]
                    .as_str()
                    .ok_or("A purpose is required")?
                    .to_owned(),
            })
        }
        "koolade_computer_use_start_session" => {
            check_keys(args, &["session_id"])?;
            Ok(Request::StartSession {
                session_id: session()?,
            })
        }
        "koolade_computer_use_observe" => {
            check_keys(args, &["session_id", "observation"])?;
            Ok(Request::Observe {
                session_id: session()?,
                observation: match args["observation"].as_str() {
                    Some("screenshot") => Observation::Screenshot,
                    Some("windows") => Observation::Windows,
                    Some("accessibility") => Observation::Accessibility,
                    Some("state") => Observation::State,
                    _ => return Err("A supported observation kind is required"),
                },
            })
        }
        "koolade_computer_use_action" => {
            check_keys(args, &["session_id", "payload", "timeout_ms"])?;
            Ok(Request::Action {
                session_id: session()?,
                payload: serde_json::from_value(args["payload"].clone())
                    .map_err(|_| "A valid Computer Use action payload is required")?,
                timeout_ms: args["timeout_ms"]
                    .as_u64()
                    .unwrap_or(30_000)
                    .try_into()
                    .map_err(|_| "Action timeout is invalid")?,
                idempotency_key: None,
            })
        }
        "koolade_computer_use_pause" => {
            check_keys(args, &["session_id"])?;
            Ok(Request::Pause {
                session_id: session()?,
            })
        }
        "koolade_computer_use_resume" => {
            check_keys(args, &["session_id"])?;
            Ok(Request::Resume {
                session_id: session()?,
            })
        }
        "koolade_computer_use_release" => {
            check_keys(args, &["session_id"])?;
            Ok(Request::Release {
                session_id: session()?,
            })
        }
        _ => Err("Computer Use tool is unavailable"),
    }
}

fn check_keys(args: &Value, allowed: &[&str]) -> Result<(), &'static str> {
    if args
        .as_object()
        .is_some_and(|object| object.keys().all(|key| allowed.contains(&key.as_str())))
    {
        Ok(())
    } else {
        Err("Unexpected Computer Use tool argument")
    }
}

#[cfg(test)]
#[path = "computer_use/tests.rs"]
mod tests;
