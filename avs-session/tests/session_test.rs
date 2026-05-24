use agentverse_session::session::{Session, SessionStatus};

#[test]
fn new_session_has_active_status() {
    let session = Session::new("user-42");
    assert_eq!(session.user_id, "user-42");
    assert!(matches!(session.status, SessionStatus::Active));
}

#[test]
fn session_id_is_unique() {
    let a = Session::new("u1");
    let b = Session::new("u1");
    assert_ne!(a.id, b.id);
}
