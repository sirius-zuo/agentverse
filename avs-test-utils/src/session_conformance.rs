//! Generic conformance suite for `SessionMemory` implementations.
//! Both the SQLite and Postgres backends must pass the identical suite —
//! this is what prevents semantic drift between them.

use agentverse::memory::{Message, MessageRole};
use agentverse_session::{SessionMemory, SessionStatus};

pub async fn run_conformance_suite<S: SessionMemory>(store: &S) {
    // create + get + ownership
    let session = store.create("alice").await.expect("create");
    let fetched = store
        .get(session.id)
        .await
        .expect("get")
        .expect("session exists");
    assert_eq!(fetched.user_id, "alice");

    // unknown session → None
    assert!(store
        .get(agentverse_session::SessionId::new_v4())
        .await
        .expect("get unknown")
        .is_none());

    // append_turn preserves order
    let user_msg = Message::text(MessageRole::User, "hi");
    let asst_msg = Message::text(MessageRole::Assistant, "hello");
    store
        .append_turn(session.id, user_msg, asst_msg)
        .await
        .expect("append_turn");
    let msgs = store.load_messages(session.id).await.expect("load");
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[0].as_text(), "hi");
    assert!(matches!(msgs[0].role, MessageRole::User));
    assert_eq!(msgs[1].as_text(), "hello");
    assert!(matches!(msgs[1].role, MessageRole::Assistant));

    // watermark semantics
    let above = store
        .load_messages_above_watermark(session.id)
        .await
        .expect("above watermark");
    assert_eq!(above.len(), 2, "all messages start above the watermark");
    let last_seq = above.last().expect("non-empty").0;
    store
        .advance_watermark(session.id, last_seq)
        .await
        .expect("advance");
    assert_eq!(
        store.get_watermark(session.id).await.expect("watermark"),
        last_seq
    );
    assert!(store
        .load_messages_above_watermark(session.id)
        .await
        .expect("above after advance")
        .is_empty());

    // skill context set / get / clear
    store
        .set_skill_context(session.id, Some(r#"{"skill":"x"}"#))
        .await
        .expect("set ctx");
    assert_eq!(
        store
            .get_skill_context(session.id)
            .await
            .expect("get ctx")
            .as_deref(),
        Some(r#"{"skill":"x"}"#)
    );
    store
        .set_skill_context(session.id, None)
        .await
        .expect("clear ctx");
    assert_eq!(
        store
            .get_skill_context(session.id)
            .await
            .expect("get ctx 2"),
        None
    );

    // interrupted state + status
    store
        .set_interrupted_state(session.id, Some(r#"{"s":1}"#))
        .await
        .expect("set interrupted");
    assert_eq!(
        store
            .get_interrupted_state(session.id)
            .await
            .expect("get interrupted")
            .as_deref(),
        Some(r#"{"s":1}"#)
    );
    store
        .update_status(session.id, SessionStatus::Interrupted)
        .await
        .expect("status");
    let after = store
        .get(session.id)
        .await
        .expect("get after status")
        .expect("session exists after status");
    assert_eq!(after.status, SessionStatus::Interrupted);

    // list_by_user isolation
    let sessions = store.list_by_user("alice").await.expect("list alice");
    assert!(sessions.iter().any(|s| s.id == session.id));
    assert!(store
        .list_by_user("bob")
        .await
        .expect("list bob")
        .is_empty());

    // list_sessions_needing_maintenance: a session with unconsolidated
    // messages appears regardless of status — this is the regression test
    // for the active-only scoping bug (a session that has ended must still
    // be visible to background workers until its messages are drained).
    let ended_with_pending = store.create("carol").await.expect("create ended session");
    store
        .append_turn(
            ended_with_pending.id,
            Message::text(MessageRole::User, "pending"),
            Message::text(MessageRole::Assistant, "reply"),
        )
        .await
        .expect("append_turn for ended session");
    store
        .update_status(ended_with_pending.id, SessionStatus::Completed)
        .await
        .expect("end the session while it still has unconsolidated messages");
    let needing_maintenance = store
        .list_sessions_needing_maintenance()
        .await
        .expect("list needing maintenance");
    assert!(
        needing_maintenance
            .iter()
            .any(|s| s.id == ended_with_pending.id),
        "an ended session with unconsolidated messages must still be visible to background workers"
    );

    // A session with everything already consolidated and no messages old
    // enough to prune should NOT appear (nothing left to do).
    let fully_drained = store.create("dave").await.expect("create drained session");
    store
        .append_turn(
            fully_drained.id,
            Message::text(MessageRole::User, "hi"),
            Message::text(MessageRole::Assistant, "hello"),
        )
        .await
        .expect("append_turn for drained session");
    let above = store
        .load_messages_above_watermark(fully_drained.id)
        .await
        .expect("load above watermark");
    let asst_seq = above.last().expect("has messages").0;
    store
        .advance_watermark(fully_drained.id, asst_seq)
        .await
        .expect("advance watermark to fully consolidate");
    let needing_maintenance_2 = store
        .list_sessions_needing_maintenance()
        .await
        .expect("list needing maintenance 2");
    assert!(
        !needing_maintenance_2
            .iter()
            .any(|s| s.id == fully_drained.id),
        "a session doesn't need re-listing just because it has old consolidated messages \
         eligible for pruning at some point — but this fixture's messages are brand new \
         (created_at = now), so they aren't past any cutoff yet either; this assertion \
         only checks that consolidation alone doesn't cause perpetual re-listing when \
         nothing is actually prunable yet in the same tick"
    );

    // delete_session: immediate, unconditional, cascades to messages.
    let to_delete = store
        .create("erin")
        .await
        .expect("create session to delete");
    store
        .append_turn(
            to_delete.id,
            Message::text(MessageRole::User, "will be deleted"),
            Message::text(MessageRole::Assistant, "ok"),
        )
        .await
        .expect("append_turn before delete");
    store
        .delete_session(to_delete.id)
        .await
        .expect("delete_session");
    assert!(
        store
            .get(to_delete.id)
            .await
            .expect("get after delete")
            .is_none(),
        "session must be gone after delete_session"
    );
    assert!(
        store
            .load_messages(to_delete.id)
            .await
            .expect("load_messages after delete")
            .is_empty(),
        "messages must be cascade-deleted along with their session (ON DELETE CASCADE) — \
         this locks in the behavior the whole retention design depends on, since neither \
         backend implementation deletes messages explicitly"
    );

    // delete_ended_sessions_before: only non-active + past-cutoff sessions
    // are removed; active sessions and not-yet-past-cutoff sessions survive.
    let old_ended = store
        .create("frank")
        .await
        .expect("create old ended session");
    store
        .update_status(old_ended.id, SessionStatus::Completed)
        .await
        .expect("end old session");
    let still_active = store
        .create("grace")
        .await
        .expect("create still-active session");
    let recently_ended = store
        .create("henry")
        .await
        .expect("create recently-ended session");
    store
        .update_status(recently_ended.id, SessionStatus::Completed)
        .await
        .expect("end recently-ended session");

    let far_future_cutoff = chrono::Utc::now().timestamp() + 86_400; // 1 day from now
    let deleted_count = store
        .delete_ended_sessions_before(far_future_cutoff)
        .await
        .expect("delete_ended_sessions_before");
    assert!(
        deleted_count >= 2,
        "both ended sessions should be deleted by a future cutoff"
    );
    assert!(
        store
            .get(old_ended.id)
            .await
            .expect("get old_ended")
            .is_none(),
        "old ended session must be deleted"
    );
    assert!(
        store
            .get(recently_ended.id)
            .await
            .expect("get recently_ended")
            .is_none(),
        "recently-ended session must be deleted too, since the cutoff is in the future"
    );
    assert!(
        store.get(still_active.id).await.expect("get still_active").is_some(),
        "an active session must NEVER be deleted by delete_ended_sessions_before, regardless of cutoff"
    );

    // A not-yet-past-cutoff ended session must survive.
    let ended_but_recent = store
        .create("iris")
        .await
        .expect("create another ended session");
    store
        .update_status(ended_but_recent.id, SessionStatus::Completed)
        .await
        .expect("end session");
    let far_past_cutoff = chrono::Utc::now().timestamp() - 86_400; // 1 day ago
    store
        .delete_ended_sessions_before(far_past_cutoff)
        .await
        .expect("delete_ended_sessions_before with a past cutoff");
    assert!(
        store
            .get(ended_but_recent.id)
            .await
            .expect("get ended_but_recent")
            .is_some(),
        "a session ended just now must survive a cutoff of 1 day ago (not old enough yet)"
    );
}
