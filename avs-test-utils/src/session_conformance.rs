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
    let user_msg = Message {
        role: MessageRole::User,
        content: "hi".into(),
    };
    let asst_msg = Message {
        role: MessageRole::Assistant,
        content: "hello".into(),
    };
    store
        .append_turn(session.id, user_msg, asst_msg)
        .await
        .expect("append_turn");
    let msgs = store.load_messages(session.id).await.expect("load");
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[0].content, "hi");
    assert!(matches!(msgs[0].role, MessageRole::User));
    assert_eq!(msgs[1].content, "hello");
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
            Message {
                role: MessageRole::User,
                content: "pending".into(),
            },
            Message {
                role: MessageRole::Assistant,
                content: "reply".into(),
            },
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
            Message {
                role: MessageRole::User,
                content: "hi".into(),
            },
            Message {
                role: MessageRole::Assistant,
                content: "hello".into(),
            },
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
}
