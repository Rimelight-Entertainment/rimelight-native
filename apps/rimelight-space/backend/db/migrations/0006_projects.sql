-- Projects and issues (PLAN section 4, slice S1; decision 0026).
-- Requires PostgreSQL 18 for uuidv7(). Primary keys are UUIDv7.
-- Every table has created_at, updated_at and deleted_at (soft delete by default).
--
-- Consistency is enforced in SQL with composite foreign keys:
--   * an issue's state and sprint belong to the issue's project;
--   * an issue's parent (subtask) is in the same project;
--   * an assignee is a member of the issue's project (SET NULL is not used: unassign first).
-- Permissions (who may see or edit) are enforced in the API through space_policy, not here.

CREATE TABLE IF NOT EXISTS projects (
    id                uuid PRIMARY KEY DEFAULT uuidv7(),
    key               text NOT NULL UNIQUE CHECK (key ~ '^[A-Z][A-Z0-9]{1,9}$'),
    name              text NOT NULL CHECK (length(btrim(name)) > 0),
    description       text NOT NULL DEFAULT '',
    lead_id           uuid NULL REFERENCES employees (id),
    next_issue_number integer NOT NULL DEFAULT 1 CHECK (next_issue_number >= 1),
    created_at        timestamptz NOT NULL DEFAULT now(),
    updated_at        timestamptz NOT NULL DEFAULT now(),
    deleted_at        timestamptz NULL
);

CREATE TABLE IF NOT EXISTS project_members (
    project_id uuid NOT NULL REFERENCES projects (id),
    user_id    uuid NOT NULL REFERENCES employees (id),
    role       text NOT NULL CHECK (role IN ('admin', 'member', 'viewer')),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    deleted_at timestamptz NULL,
    PRIMARY KEY (project_id, user_id)
);

CREATE INDEX IF NOT EXISTS project_members_user_id_idx ON project_members (user_id);

CREATE TABLE IF NOT EXISTS workflow_states (
    id         uuid PRIMARY KEY DEFAULT uuidv7(),
    project_id uuid NOT NULL REFERENCES projects (id),
    name       text NOT NULL CHECK (length(btrim(name)) > 0),
    category   text NOT NULL CHECK (category IN ('unstarted', 'started', 'done', 'cancelled')),
    position   integer NOT NULL CHECK (position >= 1),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    deleted_at timestamptz NULL,
    UNIQUE (project_id, name),
    UNIQUE (project_id, id)
);

CREATE TABLE IF NOT EXISTS sprints (
    id         uuid PRIMARY KEY DEFAULT uuidv7(),
    project_id uuid NOT NULL REFERENCES projects (id),
    name       text NOT NULL CHECK (length(btrim(name)) > 0),
    goal       text NOT NULL DEFAULT '',
    start_date date NOT NULL,
    end_date   date NOT NULL,
    state      text NOT NULL CHECK (state IN ('planned', 'active', 'closed')),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    deleted_at timestamptz NULL,
    CHECK (end_date >= start_date),
    UNIQUE (project_id, id)
);

-- At most one active sprint per project.
CREATE UNIQUE INDEX IF NOT EXISTS sprints_one_active_idx ON sprints (project_id)
    WHERE state = 'active' AND deleted_at IS NULL;

CREATE TABLE IF NOT EXISTS issues (
    id              uuid PRIMARY KEY DEFAULT uuidv7(),
    project_id      uuid NOT NULL REFERENCES projects (id),
    number          integer NOT NULL CHECK (number >= 1),
    summary         text NOT NULL CHECK (length(btrim(summary)) > 0),
    description     text NOT NULL DEFAULT '',
    type            text NOT NULL CHECK (type IN ('task', 'bug', 'feature', 'epic')),
    priority        text NOT NULL CHECK (priority IN ('critical', 'high', 'normal', 'low')),
    state_id        uuid NOT NULL,
    assignee_id     uuid NULL,
    reporter_id     uuid NOT NULL REFERENCES employees (id),
    parent_id       uuid NULL,
    sprint_id       uuid NULL,
    estimate_points integer NULL CHECK (estimate_points IS NULL OR estimate_points >= 0),
    due_date        date NULL,
    tags            text[] NOT NULL DEFAULT '{}',
    backlog_rank    text NOT NULL CHECK (length(backlog_rank) > 0),
    created_at      timestamptz NOT NULL DEFAULT now(),
    updated_at      timestamptz NOT NULL DEFAULT now(),
    resolved_at     timestamptz NULL,
    deleted_at      timestamptz NULL,
    UNIQUE (project_id, number),
    UNIQUE (project_id, id),
    CHECK (parent_id IS NULL OR parent_id <> id),
    FOREIGN KEY (project_id, state_id) REFERENCES workflow_states (project_id, id),
    FOREIGN KEY (project_id, sprint_id) REFERENCES sprints (project_id, id),
    FOREIGN KEY (project_id, parent_id) REFERENCES issues (project_id, id),
    FOREIGN KEY (project_id, assignee_id) REFERENCES project_members (project_id, user_id)
);

CREATE INDEX IF NOT EXISTS issues_project_state_idx ON issues (project_id, state_id);
CREATE INDEX IF NOT EXISTS issues_project_assignee_idx ON issues (project_id, assignee_id);
CREATE INDEX IF NOT EXISTS issues_project_sprint_idx ON issues (project_id, sprint_id);
CREATE INDEX IF NOT EXISTS issues_project_rank_idx ON issues (project_id, backlog_rank);
CREATE INDEX IF NOT EXISTS issues_parent_idx ON issues (parent_id);
CREATE INDEX IF NOT EXISTS issues_reporter_idx ON issues (reporter_id);
CREATE INDEX IF NOT EXISTS issues_tags_idx ON issues USING gin (tags);

-- One row per link. The inverse ("is blocked by", "is duplicated by") is derived by reading
-- the same row from the target side, so each pair is stored once.
CREATE TABLE IF NOT EXISTS issue_links (
    id         uuid PRIMARY KEY DEFAULT uuidv7(),
    source_id  uuid NOT NULL REFERENCES issues (id),
    target_id  uuid NOT NULL REFERENCES issues (id),
    kind       text NOT NULL CHECK (kind IN ('relates', 'blocks', 'duplicates')),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    deleted_at timestamptz NULL,
    CHECK (source_id <> target_id),
    UNIQUE (source_id, target_id, kind)
);

CREATE INDEX IF NOT EXISTS issue_links_target_idx ON issue_links (target_id);

CREATE TABLE IF NOT EXISTS comments (
    id         uuid PRIMARY KEY DEFAULT uuidv7(),
    issue_id   uuid NOT NULL REFERENCES issues (id),
    author_id  uuid NOT NULL REFERENCES employees (id),
    body       text NOT NULL CHECK (length(btrim(body)) > 0),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    deleted_at timestamptz NULL
);

CREATE INDEX IF NOT EXISTS comments_issue_created_idx ON comments (issue_id, created_at);

-- Activity log: one row per change to an issue. old_value and new_value hold JSON values.
CREATE TABLE IF NOT EXISTS activity (
    id         uuid PRIMARY KEY DEFAULT uuidv7(),
    issue_id   uuid NOT NULL REFERENCES issues (id),
    actor_id   uuid NULL REFERENCES employees (id),
    kind       text NOT NULL CHECK (kind IN ('created', 'field_changed', 'comment_added', 'link_added', 'link_removed')),
    field      text NULL,
    old_value  jsonb NULL,
    new_value  jsonb NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    deleted_at timestamptz NULL
);

CREATE INDEX IF NOT EXISTS activity_issue_created_idx ON activity (issue_id, created_at);
