use chrono::Utc;
use flowforge_core::hook::StopInput;
use flowforge_core::Result;

pub fn run() -> Result<()> {
    let ctx = super::HookContext::init()?;
    let _input = StopInput::from_value(&ctx.raw)?;

    if ctx.db.is_none() {
        return Ok(());
    }

    // Capture session BEFORE ending it (same pattern as session_end)
    let current_session = ctx.with_db("get_current_session", |db| db.get_current_session());
    let current_session = current_session.flatten();

    // End current session
    if let Some(ref session) = current_session {
        let sid = session.id.clone();
        ctx.with_db("end_session", |db| db.end_session(&sid, Utc::now()));
    }

    // Run shared learning cleanup (trajectory judgment, pattern consolidation, etc.)
    if let Some(ref session) = current_session {
        super::run_session_learning(&ctx, session);
    }

    Ok(())
}
