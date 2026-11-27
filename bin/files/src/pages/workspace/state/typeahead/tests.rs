use std::time::Duration;

use zgui::vocab::Timestamp;

use super::{TIMEOUT, TypeAhead};

fn ts(ms: u64) -> Timestamp {
    Timestamp::from_origin(Duration::from_millis(ms))
}

fn prefix_matcher<'a>(items: &'a [&'a str]) -> impl Fn(usize, &str) -> bool + 'a {
    move |idx, prefix| items[idx].to_lowercase().starts_with(prefix)
}

#[test]
fn consecutive_keys_narrow_prefix_and_reset_after_timeout() {
    let items = [
        "alpha.txt",
        "beta.txt",
        "Cargo.toml",
        "config.toml",
        "const.rs",
    ];
    let mut ta = TypeAhead::default();
    let match_item = prefix_matcher(&items);

    let mut cur = ta.push_and_match("c", ts(0), items.len(), None, &match_item);
    assert_eq!(cur, Some(2));

    cur = ta.push_and_match("o", ts(200), items.len(), cur, &match_item);
    assert_eq!(cur, Some(3));

    // Current item already matches "con", so selection stays put.
    cur = ta.push_and_match("n", ts(400), items.len(), cur, &match_item);
    assert_eq!(cur, Some(3));

    cur = ta.push_and_match("s", ts(600), items.len(), cur, &match_item);
    assert_eq!(cur, Some(4));

    // After exceeding TIMEOUT, a new key starts a fresh search and wraps around.
    let after_timeout = ts(600) + TIMEOUT + Duration::from_millis(1);
    cur = ta.push_and_match("A", after_timeout, items.len(), cur, &match_item);
    assert_eq!(cur, Some(0));
}

#[test]
fn repeated_same_character_cycles_then_switches_to_prefix_on_distinct_key() {
    let items = ["Cougar", "Leopard", "Lemur", "Lion", "Llama", "Tiger"];
    let mut ta = TypeAhead::default();
    let match_item = prefix_matcher(&items);

    let mut cur = ta.push_and_match("l", ts(0), items.len(), None, &match_item);
    assert_eq!(cur, Some(1)); // Leopard

    cur = ta.push_and_match("l", ts(200), items.len(), cur, &match_item);
    assert_eq!(cur, Some(2)); // Lemur (cycled)

    // Typing 'a' turns accumulated "ll" into "lla" -> Llama.
    cur = ta.push_and_match("a", ts(400), items.len(), cur, &match_item);
    assert_eq!(cur, Some(4)); // Llama

    // Fresh timeout + repeated 'l' wraps through all L items back to Leopard.
    let base = ts(2000);
    cur = ta.push_and_match("l", base, items.len(), Some(3), &match_item);
    assert_eq!(cur, Some(4)); // Llama
    cur = ta.push_and_match(
        "l",
        base + Duration::from_millis(150),
        items.len(),
        cur,
        &match_item,
    );
    assert_eq!(cur, Some(1)); // Leopard (wrapped)
    cur = ta.push_and_match(
        "l",
        base + Duration::from_millis(300),
        items.len(),
        cur,
        &match_item,
    );
    assert_eq!(cur, Some(2)); // Lemur
}

#[test]
fn space_is_ignored_when_idle_and_accepted_mid_query() {
    let items = ["My Documents", "My Music", "My Pictures", "Notes.txt"];
    let mut ta = TypeAhead::default();
    let match_item = prefix_matcher(&items);

    assert_eq!(
        ta.push_and_match(" ", ts(0), items.len(), None, &match_item),
        None
    );
    assert!(!ta.is_active(ts(0)));

    let mut cur = ta.push_and_match("m", ts(100), items.len(), None, &match_item);
    assert_eq!(cur, Some(0));
    cur = ta.push_and_match("y", ts(200), items.len(), cur, &match_item);
    assert_eq!(cur, Some(0));
    cur = ta.push_and_match(" ", ts(300), items.len(), cur, &match_item);
    assert_eq!(cur, Some(0));
    cur = ta.push_and_match("p", ts(400), items.len(), cur, &match_item);
    assert_eq!(cur, Some(2)); // My Pictures
}

#[test]
fn failed_sequence_does_not_jump_on_later_characters_and_backspace_recovers() {
    let items = ["alpha.txt", "beta.txt", "cargo.toml", "config.toml"];
    let mut ta = TypeAhead::default();
    let match_item = prefix_matcher(&items);

    // Typing "zebra" must not jump to "beta.txt" on 'b' or "alpha.txt" on 'a'.
    assert_eq!(
        ta.push_and_match("z", ts(0), items.len(), Some(0), &match_item),
        None
    );
    assert_eq!(
        ta.push_and_match("e", ts(100), items.len(), Some(0), &match_item),
        None
    );
    assert_eq!(
        ta.push_and_match("b", ts(200), items.len(), Some(0), &match_item),
        None
    );
    assert_eq!(
        ta.push_and_match("r", ts(300), items.len(), Some(0), &match_item),
        None
    );
    assert_eq!(
        ta.push_and_match("a", ts(400), items.len(), Some(0), &match_item),
        None
    );

    // Reset + typo recovery with Backspace: "c", "o", "x" -> Backspace -> "n" matches config.toml.
    ta.reset();
    let mut cur = ta.push_and_match("c", ts(500), items.len(), None, &match_item);
    assert_eq!(cur, Some(2));
    cur = ta.push_and_match("o", ts(600), items.len(), cur, &match_item);
    assert_eq!(cur, Some(3));
    assert_eq!(
        ta.push_and_match("x", ts(700), items.len(), cur, &match_item),
        None
    );
    assert_eq!(
        ta.pop_and_match(ts(800), items.len(), cur, &match_item),
        Some(Some(3))
    );
    cur = ta.push_and_match("n", ts(900), items.len(), cur, &match_item);
    assert_eq!(cur, Some(3));

    // Popping down to empty clears the session; subsequent Backspace is not consumed.
    ta.reset();
    assert_eq!(
        ta.pop_and_match(ts(1000), items.len(), cur, &match_item),
        None
    );
    let _ = ta.push_and_match("a", ts(1100), items.len(), None, &match_item);
    assert_eq!(
        ta.pop_and_match(ts(1200), items.len(), Some(0), &match_item),
        Some(None)
    );
    assert!(!ta.is_active(ts(1200)));
}

#[test]
fn unicode_and_multi_char_ime_commits_match_directly() {
    let items = ["Über.txt", "哈哈.txt", "哈喽.txt", "设计稿.png"];
    let mut ta = TypeAhead::default();
    let match_item = prefix_matcher(&items);

    assert_eq!(
        ta.push_and_match("ü", ts(0), items.len(), None, &match_item),
        Some(0)
    );

    // A single IME commit of a reduplicated string matches the prefix directly rather than cycling.
    let after_timeout = ts(2000);
    assert_eq!(
        ta.push_and_match("哈哈", after_timeout, items.len(), Some(0), &match_item),
        Some(1)
    );

    let next_timeout = ts(4000);
    assert_eq!(
        ta.push_and_match("设计", next_timeout, items.len(), Some(1), &match_item),
        Some(3)
    );
}
