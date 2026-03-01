#![forbid(unsafe_code)]

use serde_json::json;

use crate::commands::{CommandOutput, CommandResult};

pub fn run() -> CommandResult {
    let data = json!({
        "status": "started",
        "timestamp": let_sdk::utils::time::now_iso(),
        "message": "CLI foundation scaffold is active",
        "readyCommands": ["tools", "health", "config", "start"],
    });

    Ok(CommandOutput::new(data).with_text("start: scaffold runtime ready"))
}
