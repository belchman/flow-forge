use flowforge_core::hook::{self, NotificationInput};
use flowforge_core::Result;

pub fn run() -> Result<()> {
    let v = hook::parse_stdin_value()?;
    let _input = NotificationInput::from_value(&v)?;

    // Notifications are informational — no action needed.
    // Previously these were logged to hook-errors.log which caused
    // false "hook-err" warnings in the statusline.

    Ok(())
}
