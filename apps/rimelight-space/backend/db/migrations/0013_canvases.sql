-- Canvases (CV.1, decision 0045): personal and project whiteboards for images, notes, shapes, pen strokes and frames.
-- A canvas belongs to exactly one owner (personal, Home > Canvases) or one project. Moving a personal canvas to a
-- project clears owner_id and sets project_id; the id and the R2 keys stay the same.
-- Asset bytes live in the R2 bucket BLOB under space/canvases/{canvas_id}/assets/{sha256}.{ext}; thumbnails under
-- space/canvases/{canvas_id}/thumbs/{sha256}-{px}.webp.
-- Archiving keeps a canvas and its images (archived_at set). Deleting is permanent: the row (and by cascade its items, assets and shares) and every R2 object under its prefix are removed.
-- Not idempotent: applied once.

CREATE TABLE canvases (
    id          uuid PRIMARY KEY DEFAULT uuidv7(),
    title       text NOT NULL DEFAULT 'Untitled canvas' CHECK (length(btrim(title)) > 0),
    owner_id    uuid NULL REFERENCES employees (id),
    project_id  uuid NULL REFERENCES projects (id),
    created_by  uuid NOT NULL REFERENCES employees (id),
    seq         bigint NOT NULL DEFAULT 0 CHECK (seq >= 0),   -- last op sequence applied by the live room
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now(),
    archived_at timestamptz NULL,
    CONSTRAINT canvases_one_home CHECK ((owner_id IS NULL) <> (project_id IS NULL))
);

CREATE INDEX canvases_owner_idx ON canvases (owner_id) WHERE archived_at IS NULL;
CREATE INDEX canvases_project_idx ON canvases (project_id) WHERE archived_at IS NULL;

-- Personal canvases only. The API enforces this: a canvas with a project_id has no rows here.
CREATE TABLE canvas_shares (
    canvas_id   uuid NOT NULL REFERENCES canvases (id) ON DELETE CASCADE,
    user_id     uuid NOT NULL REFERENCES employees (id),
    role        text NOT NULL CHECK (role IN ('Viewer', 'Editor')),
    created_by  uuid NOT NULL REFERENCES employees (id),
    created_at  timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (canvas_id, user_id)
);

CREATE INDEX canvas_shares_user_idx ON canvas_shares (user_id);

CREATE TABLE canvas_items (
    canvas_id   uuid NOT NULL REFERENCES canvases (id) ON DELETE CASCADE,
    id          uuid NOT NULL,   -- chosen by the client
    kind        text NOT NULL CHECK (kind IN ('Image', 'Text', 'Shape', 'Stroke', 'Frame')),
    z           text COLLATE "C" NOT NULL,   -- fractional rank; items draw in ascending z
    version     bigint NOT NULL DEFAULT 1 CHECK (version >= 1),
    props       jsonb NOT NULL DEFAULT '{}'::jsonb,
    deleted     boolean NOT NULL DEFAULT false,
    created_by  uuid NOT NULL REFERENCES employees (id),
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_by  uuid NOT NULL REFERENCES employees (id),
    updated_at  timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (canvas_id, id)
);

CREATE INDEX canvas_items_live_idx ON canvas_items (canvas_id, z) WHERE NOT deleted;

CREATE TABLE canvas_assets (
    id            uuid PRIMARY KEY DEFAULT uuidv7(),
    canvas_id     uuid NOT NULL REFERENCES canvases (id) ON DELETE CASCADE,
    sha256        text NOT NULL CHECK (sha256 ~ '^[0-9a-f]{64}$'),
    mime          text NOT NULL CHECK (mime IN ('image/png', 'image/jpeg', 'image/webp', 'image/gif')),
    bytes         bigint NOT NULL CHECK (bytes > 0 AND bytes <= 52428800),   -- 50 MB per file
    width         integer NOT NULL CHECK (width > 0),
    height        integer NOT NULL CHECK (height > 0),
    frame_count   integer NOT NULL DEFAULT 1 CHECK (frame_count >= 1),   -- more than 1 for animated GIF and WebP
    file_name     text NOT NULL DEFAULT '',
    source_url    text NOT NULL DEFAULT '',   -- set when imported from a URL
    object_key    text NOT NULL,
    thumb_keys    jsonb NOT NULL DEFAULT '{}'::jsonb,   -- long-edge pixel size as text -> R2 key, e.g. {"256": "...", "1024": "..."}
    created_by    uuid NOT NULL REFERENCES employees (id),
    created_at    timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT canvas_assets_unique_hash UNIQUE (canvas_id, sha256)   -- a file is stored once per canvas
);

-- A missing row means the default: 1 GB for a user's personal canvases, 10 GB for a project.
-- Usage is the sum of canvas_assets.bytes over all the scope's canvases, archived ones included (their images are kept). Deleting a canvas removes its rows and its R2 objects, which frees the space.
CREATE TABLE storage_quotas (
    scope        text NOT NULL CHECK (scope IN ('User', 'Project')),
    subject_id   uuid NOT NULL,   -- employees.id for User, projects.id for Project
    limit_bytes  bigint NOT NULL CHECK (limit_bytes > 0),
    updated_by   uuid NOT NULL REFERENCES employees (id),
    updated_at   timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (scope, subject_id)
);
