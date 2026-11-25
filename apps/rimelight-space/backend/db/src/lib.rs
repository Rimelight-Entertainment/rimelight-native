//! rimelight_backend_db: Postgres schema for the backend.
//!
//! The migration SQL is embedded so that the backend and tests can apply it
//! without reading files at runtime. Apply them in order: 0001, 0002, 0003, 0005, 0006, 0007, 0008.

/// Creates the `users`, `sessions` and `login_codes` tables.
pub const MIGRATION_0001_AUTH: &str = include_str!("../migrations/0001_auth.sql");

/// Seeds the first user when `users` is empty. Needs the psql variable `owner_email`.
pub const MIGRATION_0002_SEED_OWNER: &str = include_str!("../migrations/0002_seed_owner.sql");

/// Adds the people profile and org columns to `users`, and creates `departments` and `teams`.
pub const MIGRATION_0003_PEOPLE: &str = include_str!("../migrations/0003_people.sql");

/// Adds department parents and heads, job positions, `users.position_id` and `user_roles`
/// (owner addendum to decision 0019). Runs once: the dev server checks for `users.position_id` first.
pub const MIGRATION_0005_ORG_ROLES: &str = include_str!("../migrations/0005_org_roles.sql");

/// Creates the project, membership, workflow state, sprint, issue, link, comment and activity
/// tables (PLAN section 4, S1; decision 0026). Needs PostgreSQL 18 for `uuidv7()`.
pub const MIGRATION_0006_PROJECTS: &str = include_str!("../migrations/0006_projects.sql");

/// Adds `issues.updated_by`, and the `issue_votes`, `issue_stars`, `issue_work_items` and
/// `issue_attachments` tables (PLAN section 4, S1.5a). Runs once: the dev server checks for `issue_votes` first.
pub const MIGRATION_0007_ISSUE_EXTRAS: &str = include_str!("../migrations/0007_issue_extras.sql");

/// Creates the `seats` table and the team budget columns (decision 0036, PLAN section 4, SP.5).
/// Runs once: the dev server checks for the `seats` table first.
pub const MIGRATION_0008_SEATS: &str = include_str!("../migrations/0008_seats.sql");

/// Adds `projects.image_key`: the picture of a project in the R2 bucket (space/projects/), or NULL.
pub const MIGRATION_0009_PROJECT_IMAGES: &str =
    include_str!("../migrations/0009_project_images.sql");

/// Positions are titles, seats are slots (decision 0042): salary moves to positions, seats lose title and salary.
pub const MIGRATION_0010_POSITIONS_ARE_TITLES: &str =
    include_str!("../migrations/0010_positions_are_titles.sql");
/// Creates the `locations` table: company offices, sites and remote hubs (Company > Locations).
/// Not idempotent: applied once.
pub const MIGRATION_0011_LOCATIONS: &str = include_str!("../migrations/0011_locations.sql");
/// Capitalises the enum-like values (issue type and priority, workflow category, sprint state) and
/// replaces their CHECK constraints. Not idempotent: applied once.
pub const MIGRATION_0012_CAPITALISED_ENUMS: &str =
    include_str!("../migrations/0012_capitalised_enums.sql");

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn projects_migration_creates_every_table_with_the_key_constraints() {
        for needle in [
            "id                uuid PRIMARY KEY DEFAULT uuidv7()",
            "CREATE TABLE IF NOT EXISTS projects",
            "CHECK (key ~ '^[A-Z][A-Z0-9]{1,9}$')",
            "CREATE TABLE IF NOT EXISTS project_members",
            "role IN ('admin', 'member', 'viewer')",
            "CREATE TABLE IF NOT EXISTS workflow_states",
            "category IN ('unstarted', 'started', 'done', 'cancelled')",
            "UNIQUE (project_id, name)",
            "CREATE TABLE IF NOT EXISTS sprints",
            "state IN ('planned', 'active', 'closed')",
            "CREATE TABLE IF NOT EXISTS issues",
            "UNIQUE (project_id, number)",
            "type IN ('task', 'bug', 'feature', 'epic')",
            "priority IN ('critical', 'high', 'normal', 'low')",
            "FOREIGN KEY (project_id, state_id) REFERENCES workflow_states (project_id, id)",
            "FOREIGN KEY (project_id, assignee_id) REFERENCES project_members (project_id, user_id)",
            "FOREIGN KEY (project_id, parent_id) REFERENCES issues (project_id, id)",
            "backlog_rank    text NOT NULL",
            "CREATE TABLE IF NOT EXISTS issue_links",
            "kind IN ('relates', 'blocks', 'duplicates')",
            "CREATE TABLE IF NOT EXISTS comments",
            "CREATE TABLE IF NOT EXISTS activity",
            "kind IN ('created', 'field_changed', 'comment_added', 'link_added', 'link_removed')",
        ] {
            assert!(
                MIGRATION_0006_PROJECTS.contains(needle),
                "missing: {needle}"
            );
        }
    }

    #[test]
    fn capitalised_enums_migration_converts_and_rechecks_the_values() {
        for needle in [
            "UPDATE issues SET type = initcap(type)",
            "lower(priority) IN ('critical', 'high', 'normal', 'low')",
            "CHECK (type IN ('Task', 'Bug', 'Feature', 'Epic'))",
            "CHECK (category IN ('Unstarted', 'Started', 'Done', 'Cancelled'))",
            "WHERE state = 'Active' AND deleted_at IS NULL",
        ] {
            assert!(
                MIGRATION_0012_CAPITALISED_ENUMS.contains(needle),
                "missing: {needle}"
            );
        }
    }

    #[test]
    fn issue_extras_migration_adds_the_tables_and_the_updated_by_column() {
        for needle in [
            "ALTER TABLE issues ADD COLUMN IF NOT EXISTS updated_by uuid NULL REFERENCES employees (id)",
            "CREATE TABLE IF NOT EXISTS issue_votes",
            "CREATE TABLE IF NOT EXISTS issue_stars",
            "PRIMARY KEY (issue_id, user_id)",
            "CREATE TABLE IF NOT EXISTS issue_work_items",
            "minutes    integer NOT NULL CHECK (minutes > 0)",
            "CREATE TABLE IF NOT EXISTS issue_attachments",
            "blob_key     text NOT NULL UNIQUE",
        ] {
            assert!(
                MIGRATION_0007_ISSUE_EXTRAS.contains(needle),
                "missing: {needle}"
            );
        }
    }

    #[test]
    fn people_migration_creates_org_tables_and_profile_columns() {
        for needle in [
            "ALTER COLUMN id SET DEFAULT gen_random_uuid()",
            "CREATE TABLE IF NOT EXISTS departments",
            "CREATE TABLE IF NOT EXISTS teams",
            "lead_id       uuid NULL REFERENCES employees (id)",
            "manager_id        uuid NULL REFERENCES employees (id)",
            "is_admin          boolean NOT NULL DEFAULT false",
        ] {
            assert!(MIGRATION_0003_PEOPLE.contains(needle), "missing: {needle}");
        }
    }
}
