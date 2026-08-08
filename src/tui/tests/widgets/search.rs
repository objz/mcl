use super::*;
use crossterm::event::KeyModifiers;

#[test]
fn confirm_keeps_query_but_deactivates() {
    let mut s = SearchState::default();
    s.activate();
    s.push('a');
    s.push('b');
    s.confirm();
    assert!(!s.active);
    assert_eq!(s.query, "ab");
    // filter should still match
    assert!(s.matches("abc"));
    assert!(!s.matches("xyz"));
    s.activate();
    assert!(s.active);
    assert_eq!(s.query, "ab");
}

#[test]
fn deactivate_clears_query() {
    let mut s = SearchState::default();
    s.activate();
    s.push('x');
    s.deactivate();
    assert!(!s.active);
    assert!(s.query.is_empty());
    // with empty query, everything matches
    assert!(s.matches("anything"));
}

#[test]
fn highlight_spans_marks_each_case_insensitive_match() {
    let search = SearchState {
        query: "di".to_owned(),
        ..SearchState::default()
    };

    let spans = search.highlight_spans("Discovery DISK", Style::default());

    assert_eq!(
        spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>(),
        "Discovery DISK"
    );
    assert!(spans[0].style.add_modifier.contains(Modifier::BOLD));
    assert!(spans[0].style.add_modifier.contains(Modifier::UNDERLINED));
    assert!(spans[2].style.add_modifier.contains(Modifier::BOLD));
    assert!(spans[2].style.add_modifier.contains(Modifier::UNDERLINED));
}

#[test]
fn ctrl_backspace_deletes_the_previous_word() {
    let mut search = SearchState {
        query: "alpha βeta  ".to_owned(),
        ..SearchState::default()
    };

    search.backspace(KeyModifiers::CONTROL);

    assert_eq!(search.query, "alpha ");
}

#[test]
fn deleting_a_word_before_the_cursor_preserves_the_suffix() {
    let mut text = "hello brave world".to_owned();
    let cursor = "hello brave".chars().count();

    let cursor = delete_previous_word(&mut text, cursor);

    assert_eq!(text, "hello  world");
    assert_eq!(cursor, "hello ".chars().count());
}
