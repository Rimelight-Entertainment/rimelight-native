//! Habits wire format, checked against responses captured from the local backend
//! (`fixtures/habits_list.raw`) and the Solid client's action keys.

use rimelight_personal_core::ApiError;
use rimelight_personal_core::habits::{HabitAction, decode_habits};
use serde_json::json;

const LIST: &[u8] = include_bytes!("fixtures/habits_list.raw");

#[test]
fn list_decodes_years_categories_and_completed_dates() {
    let data = decode_habits(LIST).expect("list fixture decodes");
    assert_eq!(data.years.len(), 1);
    let year = &data.years[0];
    assert_eq!(year.year, 2026);
    let category = &year.categories[0];
    assert_eq!(category.id, "b277463c-caea-429a-bf17-69f67570786c");
    assert_eq!(category.name, "Health");
    let habit = &category.habits[0];
    assert_eq!(habit.id, "54988184-cbf0-4449-a4e1-f48285178464");
    assert_eq!(habit.name, "Run");
    assert_eq!(habit.completed_dates, vec!["2026-09-13".to_owned()]);
}

#[test]
fn empty_tracker_decodes_to_no_years() {
    let data = decode_habits(br#"{"years":[]}"#).expect("empty tracker decodes");
    assert!(data.years.is_empty());
}

#[test]
fn body_without_years_is_a_decode_error_not_an_empty_tracker() {
    // A 200 body that isn't the blob must not silently wipe the local tracker.
    assert!(matches!(
        decode_habits(br#"{"error":"boom"}"#),
        Err(ApiError::Decode(_))
    ));
}

#[test]
fn action_bodies_use_the_keys_the_solid_client_sends() {
    let cases = [
        (
            HabitAction::AddYear { year: 2026 },
            json!({"action": "addYear", "year": 2026}),
        ),
        (
            HabitAction::AddCategory {
                year: 2026,
                name: "Health",
            },
            json!({"action": "addCategory", "year": 2026, "name": "Health"}),
        ),
        (
            HabitAction::DeleteCategory {
                year: 2026,
                cat_id: "c1",
            },
            json!({"action": "deleteCategory", "year": 2026, "catId": "c1"}),
        ),
        (
            HabitAction::AddHabit {
                year: 2026,
                cat_id: "c1",
                name: "Run",
            },
            json!({"action": "addHabit", "year": 2026, "catId": "c1", "name": "Run"}),
        ),
        (
            HabitAction::DeleteHabit {
                year: 2026,
                cat_id: "c1",
                habit_id: "h1",
            },
            json!({"action": "deleteHabit", "year": 2026, "catId": "c1", "habitId": "h1"}),
        ),
        (
            HabitAction::ToggleDate {
                year: 2026,
                cat_id: "c1",
                habit_id: "h1",
                date: "2026-09-13",
            },
            json!({
                "action": "toggleDate",
                "year": 2026,
                "catId": "c1",
                "habitId": "h1",
                "date": "2026-09-13"
            }),
        ),
    ];
    for (action, expected) in cases {
        assert_eq!(serde_json::to_value(&action).expect("serializes"), expected);
    }
}
