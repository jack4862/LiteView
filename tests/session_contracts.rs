use std::path::PathBuf;

use liteview::session::SessionState;

#[test]
fn accepts_only_the_latest_request_for_the_current_image() {
    let mut session = SessionState::default();
    let first = session.begin(PathBuf::from("first.png"));
    let second = session.begin(PathBuf::from("second.png"));

    assert!(!session.accepts(first, &PathBuf::from("first.png")));
    assert!(session.accepts(second, &PathBuf::from("second.png")));
    assert!(!session.accepts(second, &PathBuf::from("first.png")));
}

#[test]
fn reload_invalidates_an_in_flight_result_for_the_same_path() {
    let mut session = SessionState::default();
    let path = PathBuf::from("photo.jpg");
    let stale = session.begin(path.clone());
    let current = session.begin(path.clone());

    assert!(!session.accepts(stale, &path));
    assert!(session.accepts(current, &path));
}

#[test]
fn adjacent_preloads_are_cacheable_only_within_the_current_generation() {
    let mut session = SessionState::default();
    let stale = session.begin(PathBuf::from("current.jpg"));
    let current = session.begin(PathBuf::from("current.jpg"));

    assert!(session.is_current_generation(current));
    assert!(!session.is_current_generation(stale));
    assert!(!session.accepts(current, &PathBuf::from("next.jpg")));
    assert_eq!(
        session.current_path(),
        Some(PathBuf::from("current.jpg").as_path())
    );
}
