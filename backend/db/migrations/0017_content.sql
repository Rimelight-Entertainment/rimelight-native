-- Content (CMS.3, decision 0049): the main Loro document per page, author drafts with their members, and comments
-- anchored to blocks. The page scope and visibility columns on `pages` come from the CMS package's own migration
-- 0014_space_scope_and_visibility (the package owns `pages`), which must run before this one.
-- Not idempotent: applied once.

-- The page's main (published line) Loro document.
CREATE TABLE content_docs (
    page_id     uuid PRIMARY KEY REFERENCES pages (id) ON DELETE CASCADE,
    loro        bytea NOT NULL,
    updated_at  timestamptz NOT NULL DEFAULT now()
);

-- One draft per author per page while it is open or in review.
CREATE TABLE content_drafts (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    page_id     uuid NOT NULL REFERENCES pages (id) ON DELETE CASCADE,
    author_id   uuid NOT NULL REFERENCES employees (id),
    status      text NOT NULL DEFAULT 'Open' CHECK (status IN ('Open', 'InReview', 'Published', 'Discarded')),
    loro        bytea NULL,
    version_id  uuid NULL REFERENCES page_versions (id),
    summary     text NULL,
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now(),
    closed_at   timestamptz NULL
);

CREATE UNIQUE INDEX content_drafts_one_open_per_author_idx
    ON content_drafts (page_id, author_id)
    WHERE status IN ('Open', 'InReview');

CREATE TABLE content_draft_members (
    draft_id    uuid NOT NULL REFERENCES content_drafts (id) ON DELETE CASCADE,
    user_id     uuid NOT NULL REFERENCES employees (id),
    added_at    timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (draft_id, user_id)
);

CREATE TABLE content_comments (
    id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    page_id      uuid NOT NULL REFERENCES pages (id) ON DELETE CASCADE,
    draft_id     uuid NULL REFERENCES content_drafts (id) ON DELETE CASCADE,
    block_id     text NULL,
    anchor       jsonb NULL,
    parent_id    uuid NULL REFERENCES content_comments (id),
    author_id    uuid NOT NULL REFERENCES employees (id),
    body         text NOT NULL CHECK (length(btrim(body)) > 0),
    created_at   timestamptz NOT NULL DEFAULT now(),
    updated_at   timestamptz NOT NULL DEFAULT now(),
    resolved_at  timestamptz NULL,
    resolved_by  uuid NULL REFERENCES employees (id),
    deleted_at   timestamptz NULL
);

CREATE INDEX content_comments_page_idx ON content_comments (page_id, draft_id);

-- Company pages are edited by global admins and holders of the new content_editor role (decision 0049).
ALTER TABLE user_roles DROP CONSTRAINT IF EXISTS user_roles_role_check;
ALTER TABLE user_roles ADD CONSTRAINT user_roles_role_check CHECK (role IN ('admin', 'hr', 'content_editor'));
