//! Icons (P0.14): our [`IconName`] for the shell's needs, mapped to the Lucide icons bundled in rimelight-rs
//! (`crates/rimelight_ui/assets/icons/`, ISC licence, see `PLAN.md section 8.7`). The shared `ui/icons.slint` loads the same
//! files; `slint_ui::assets` checks that its names match the Rust lists.

/// The icons the shell needs now. Each maps to one bundled Lucide icon, listed in [`IconName::lucide`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconName {
    Home,
    Chat,
    Mail,
    Calendar,
    Projects,
    Company,
    People,
    Finance,
    Customers,
    Operations,
    Analytics,
    Admin,
    Search,
    Bell,
    Help,
    Plus,
    ChevronLeft,
    ChevronRight,
    ChevronsUpDown,
    Close,
    Check,
    More,
    SidebarToggle,
    Sun,
    Moon,
    Monitor,
    ExternalLink,
    /// Feedback tone icons (P0.16 batch 3): info, success, warning and danger.
    Info,
    CircleCheck,
    TriangleAlert,
    CircleX,
    /// Shell (P0.24): collapse chevron of a sidebar section, and the split-right action.
    ChevronDown,
    Split,
}

impl IconName {
    /// Every mapped icon, in declaration order.
    pub const ALL: [IconName; 33] = [
        Self::Home,
        Self::Chat,
        Self::Mail,
        Self::Calendar,
        Self::Projects,
        Self::Company,
        Self::People,
        Self::Finance,
        Self::Customers,
        Self::Operations,
        Self::Analytics,
        Self::Admin,
        Self::Search,
        Self::Bell,
        Self::Help,
        Self::Plus,
        Self::ChevronLeft,
        Self::ChevronRight,
        Self::ChevronsUpDown,
        Self::Close,
        Self::Check,
        Self::More,
        Self::SidebarToggle,
        Self::Sun,
        Self::Moon,
        Self::Monitor,
        Self::ExternalLink,
        Self::Info,
        Self::CircleCheck,
        Self::TriangleAlert,
        Self::CircleX,
        Self::ChevronDown,
        Self::Split,
    ];

    /// The Lucide name in the bundled set (file `icons/<name>.svg`).
    pub fn lucide(self) -> &'static str {
        match self {
            Self::Home => "house",
            Self::Chat => "message-square",
            Self::Mail => "mail",
            Self::Calendar => "calendar",
            Self::Projects => "folder-kanban",
            Self::Company => "building-2",
            Self::People => "users",
            Self::Finance => "landmark",
            Self::Customers => "headset",
            Self::Operations => "boxes",
            Self::Analytics => "chart-column",
            Self::Admin => "shield",
            Self::Search => "search",
            Self::Bell => "bell",
            Self::Help => "circle-question-mark",
            Self::Plus => "plus",
            Self::ChevronLeft => "chevron-left",
            Self::ChevronRight => "chevron-right",
            Self::ChevronsUpDown => "chevrons-up-down",
            Self::Close => "x",
            Self::Check => "check",
            Self::More => "ellipsis",
            Self::SidebarToggle => "panel-left",
            Self::Sun => "sun",
            Self::Moon => "moon",
            Self::Monitor => "monitor",
            Self::ExternalLink => "external-link",
            Self::Info => "info",
            Self::CircleCheck => "circle-check",
            Self::TriangleAlert => "triangle-alert",
            Self::CircleX => "circle-x",
            Self::ChevronDown => "chevron-down",
            Self::Split => "columns-2",
        }
    }
}

/// The three icon sizes (tokens: 16, 20, 24 px).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IconSize {
    Small,
    #[default]
    Medium,
    Large,
}

impl IconSize {
    /// Logical pixels, from the size tokens.
    pub fn px(self) -> f32 {
        use rimelight_design::tokens::icon;
        match self {
            Self::Small => icon::SMALL,
            Self::Medium => icon::MEDIUM,
            Self::Large => icon::LARGE,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_names_are_unique() {
        let mut names: Vec<&str> = IconName::ALL.iter().map(|n| n.lucide()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), IconName::ALL.len());
    }

    #[test]
    fn sizes_are_the_three_tokens() {
        let px: Vec<f32> = [IconSize::Small, IconSize::Medium, IconSize::Large]
            .iter()
            .map(|s| s.px())
            .collect();
        assert_eq!(px, vec![16.0, 20.0, 24.0]);
        assert_eq!(IconSize::default(), IconSize::Medium);
    }
}
