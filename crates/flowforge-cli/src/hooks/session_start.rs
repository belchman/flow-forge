use flowforge_core::hook::{self, ContextOutput, SessionStartInput};
use flowforge_core::Result;

pub fn run() -> Result<()> {
    // Drain stdin (required — Claude Code sends JSON on stdin)
    let v = hook::parse_stdin_value()?;
    let _input = SessionStartInput::from_value(&v)?;
    let output = ContextOutput::with_context("[FlowForge] Ready.".to_string());
    hook::write_stdout(&output)?;
    Ok(())
}
