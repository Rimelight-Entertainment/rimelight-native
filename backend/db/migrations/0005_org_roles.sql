-- Org structure, job positions and permission roles (decision 0019, owner addendum 2026-11-15).
-- Not idempotent: the dev server applies it only when users.position_id is missing.

ALTER TABLE departments
    ADD COLUMN IF NOT EXISTS parent_department_id uuid NULL REFERENCES departments (id),
    ADD COLUMN IF NOT EXISTS head_id              uuid NULL REFERENCES employees (id),
    ADD CONSTRAINT departments_not_own_parent CHECK (parent_department_id IS NULL OR parent_department_id <> id);

CREATE TABLE IF NOT EXISTS positions (
    id            uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    title         text NOT NULL,
    department_id uuid NULL REFERENCES departments (id),
    level         text NOT NULL DEFAULT '',
    description   text NOT NULL DEFAULT '',
    created_at    timestamptz NOT NULL DEFAULT now()
);

ALTER TABLE employees ADD COLUMN IF NOT EXISTS position_id uuid NULL REFERENCES positions (id);
CREATE INDEX IF NOT EXISTS employees_position_id_idx ON employees (position_id);

-- Built-in roles only: 'admin' and 'hr'. Custom roles are out of scope.
CREATE TABLE IF NOT EXISTS user_roles (
    user_id    uuid NOT NULL REFERENCES employees (id),
    role       text NOT NULL CHECK (role IN ('admin', 'hr')),
    granted_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, role)
);

-- users.is_admin is kept as a legacy column. Its value is copied once here; the API reads roles only.
INSERT INTO user_roles (user_id, role)
SELECT id, 'admin' FROM employees WHERE is_admin
ON CONFLICT DO NOTHING;
