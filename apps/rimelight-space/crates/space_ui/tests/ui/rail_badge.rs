//! OQ3 rail work-queue badges. The badges are counts only, drawn on the rail item of each queue area: Chat (unread),
//! People (waiting), Finance (due), Customers (unassigned) and Operations (waiting). A count of 0 draws no badge, a count
//! above 99 draws "99+", and an area without a queue ignores its count. The data is static mock data from the shell's
//! `rail-badges` property. The test drives a `Rail` on its own (the same component the shell places), with the badge
//! model from `rail_badges`, and reads what the rail draws through the rail's test hooks.

use slint::{ModelRc, VecModel};
use space_ui::slint_ui::shell::rail_badges;

pub use space_ui_test_slint::rail_badge::*;

/// The areas of `Area::ALL`, in order: Home, Chat, Projects, Company, People, Finance, Customers, Operations, Analytics, Admin.
const AREAS: [&str; 10] = [
    "Home",
    "Chat",
    "Projects",
    "Company",
    "People",
    "Finance",
    "Customers",
    "Operations",
    "Analytics",
    "Admin",
];

/// Sets the rail's badges from per-area counts, in `Area::ALL` order.
fn set_counts(form: &RailForm, counts: &[u32]) {
    let badges: Vec<RailBadge> = rail_badges(counts)
        .into_iter()
        .map(|badge| RailBadge {
            count: badge.count,
            text: badge.text,
            noun: badge.noun,
        })
        .collect();
    form.set_badges(ModelRc::new(VecModel::from(badges)));
}

/// The texts of every badge the rail draws, in area order.
fn badge_texts(form: &RailForm) -> Vec<String> {
    (0..AREAS.len() as i32)
        .map(|index| form.invoke_test_badge_text(index).to_string())
        .filter(|text| !text.is_empty())
        .collect()
}

/// The accessible label the rail gives area `index`.
fn area_label(form: &RailForm, index: i32) -> String {
    form.invoke_test_area_label(index).to_string()
}

pub fn rail_badges_show_counts_and_only_for_queue_areas() {
    crate::support::init();
    let form = RailForm::new().expect("rail form builds");
    let items: Vec<RailItem> = AREAS
        .iter()
        .map(|label| RailItem {
            label: (*label).into(),
            icon: slint::Image::default(),
        })
        .collect();
    form.set_items(ModelRc::new(VecModel::from(items)));

    // The default: no badge at all, and the rail labels are the plain area names.
    assert!(badge_texts(&form).is_empty(), "no badge by default");
    assert_eq!(area_label(&form, 1), "Chat");
    assert_eq!(area_label(&form, 5), "Finance");

    // A count of 0 draws no badge, and a nonzero count on an area without a queue (Home, Projects) draws none either.
    set_counts(&form, &[7, 0, 9, 0, 0, 0, 0, 0, 0, 0]);
    assert!(
        badge_texts(&form).is_empty(),
        "Home and Projects have no queue"
    );
    assert_eq!(
        area_label(&form, 0),
        "Home",
        "no count in the label of Home"
    );

    // Counts 1, 12 and 150 on Chat, People and Finance: the badge text is 1, 12 and 99+.
    set_counts(&form, &[0, 1, 0, 0, 12, 150, 0, 0, 0, 0]);
    let mut texts = badge_texts(&form);
    texts.sort();
    assert_eq!(texts, ["1", "12", "99+"]);

    // The accessible label of each queue area carries its count and noun.
    assert_eq!(area_label(&form, 1), "Chat, 1 unread");
    assert_eq!(area_label(&form, 4), "People, 12 waiting");
    assert_eq!(area_label(&form, 5), "Finance, 99+ due");
    assert_ne!(
        area_label(&form, 1),
        "Chat",
        "the plain label is replaced when a badge shows"
    );
}
