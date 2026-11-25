-- Issue extras (PLAN section 4, slice S1.5a): votes, stars, spent time, attachments and who last
-- changed an issue. Needs PostgreSQL 18 for uuidv7(), as 0006 does. Additive: applied once by the
-- dev server, which checks for `issue_votes` first (ensure_table).
-- Permissions (who may vote, edit or read) are enforced in the API through space_policy, not here.

ALTER TABLE issues ADD COLUMN IF NOT EXISTS updated_by uuid NULL REFERENCES employees (id);

-- One row per user who voted for or starred an issue. Removing the vote deletes the row.
CREATE TABLE IF NOT EXISTS issue_votes (
    issue_id   uuid NOT NULL REFERENCES issues (id),
    user_id    uuid NOT NULL REFERENCES employees (id),
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (issue_id, user_id)
);

CREATE TABLE IF NOT EXISTS issue_stars (
    issue_id   uuid NOT NULL REFERENCES issues (id),
    user_id    uuid NOT NULL REFERENCES employees (id),
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (issue_id, user_id)
);

-- Spent time. Soft delete, as the other tables. `minutes` is always positive.
CREATE TABLE IF NOT EXISTS issue_work_items (
    id         uuid PRIMARY KEY DEFAULT uuidv7(),
    issue_id   uuid NOT NULL REFERENCES issues (id),
    author_id  uuid NOT NULL REFERENCES employees (id),
    minutes    integer NOT NULL CHECK (minutes > 0),
    spent_on   date NOT NULL,
    note       text NOT NULL DEFAULT '',
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    deleted_at timestamptz NULL
);

CREATE INDEX IF NOT EXISTS issue_work_items_issue_idx ON issue_work_items (issue_id);

-- Files attached to an issue. The bytes live in the blob store under `blob_key`.
CREATE TABLE IF NOT EXISTS issue_attachments (
    id           uuid PRIMARY KEY DEFAULT uuidv7(),
    issue_id     uuid NOT NULL REFERENCES issues (id),
    uploader_id  uuid NOT NULL REFERENCES employees (id),
    file_name    text NOT NULL CHECK (length(btrim(file_name)) > 0),
    content_type text NOT NULL,
    size_bytes   bigint NOT NULL CHECK (size_bytes >= 0),
    blob_key     text NOT NULL UNIQUE,
    created_at   timestamptz NOT NULL DEFAULT now(),
    updated_at   timestamptz NOT NULL DEFAULT now(),
    deleted_at   timestamptz NULL
);

CREATE INDEX IF NOT EXISTS issue_attachments_issue_idx ON issue_attachments (issue_id);
