-- Project images (owner, 2026-11-16). The picture lives in the shared R2 bucket under
-- space/projects/; this column holds its key, or NULL when the project has no picture.
ALTER TABLE projects ADD COLUMN IF NOT EXISTS image_key text NULL;
