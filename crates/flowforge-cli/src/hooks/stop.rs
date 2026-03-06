use chrono::Utc;
use flowforge_core::hook::StopInput;
use flowforge_core::Result;

pub fn run() -> Result<()> {
    let ctx = super::HookContext::init()?;
    let _input = StopInput::from_value(&ctx.raw)?;

    if ctx.db.is_none() {
        return Ok(());
    }

    // Use the specific session ID from the hook input (avoids ending the wrong session)
    let current_session = if let Some(ref sid) = ctx.session_id {
        ctx.with_db("get_session_by_id", |db| db.get_session_by_id(sid))
            .flatten()
    } else {
        ctx.with_db("get_current_session", |db| db.get_current_session())
            .flatten()
    };

    // End the specific session
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
