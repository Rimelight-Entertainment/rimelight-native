-- People: profiles, org structure and the admin flag (decision 0019).
-- Work fields are visible to every employee. Personal contact fields (personal_phone,
-- personal_email, address, birthday, emergency_contact) are visible only to the person,
-- their manager chain, and admins and HR. Enforced in the API, not in SQL.

ALTER TABLE employees ALTER COLUMN id SET DEFAULT gen_random_uuid();

CREATE TABLE IF NOT EXISTS departments (
    id         uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    name       text NOT NULL UNIQUE,
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS teams (
    id            uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    name          text NOT NULL,
    department_id uuid NOT NULL REFERENCES departments (id),
    lead_id       uuid NULL REFERENCES employees (id),
    created_at    timestamptz NOT NULL DEFAULT now(),
    UNIQUE (department_id, name)
);

ALTER TABLE employees
    ADD COLUMN IF NOT EXISTS preferred_name    text NULL,
    ADD COLUMN IF NOT EXISTS pronouns          text NULL,
    ADD COLUMN IF NOT EXISTS title             text NULL,
    ADD COLUMN IF NOT EXISTS department_id     uuid NULL REFERENCES departments (id),
    ADD COLUMN IF NOT EXISTS team_id           uuid NULL REFERENCES teams (id),
    ADD COLUMN IF NOT EXISTS manager_id        uuid NULL REFERENCES employees (id),
    ADD COLUMN IF NOT EXISTS location          text NULL,
    ADD COLUMN IF NOT EXISTS timezone          text NULL,
    ADD COLUMN IF NOT EXISTS work_phone        text NULL,
    ADD COLUMN IF NOT EXISTS bio               text NULL,
    ADD COLUMN IF NOT EXISTS skills            text[] NOT NULL DEFAULT '{}',
    ADD COLUMN IF NOT EXISTS start_date        date NULL,
    ADD COLUMN IF NOT EXISTS avatar_key        text NULL,
    ADD COLUMN IF NOT EXISTS is_admin          boolean NOT NULL DEFAULT false,
    ADD COLUMN IF NOT EXISTS personal_phone    text NULL,
    ADD COLUMN IF NOT EXISTS personal_email    text NULL,
    ADD COLUMN IF NOT EXISTS address           text NULL,
    ADD COLUMN IF NOT EXISTS birthday          date NULL,
    ADD COLUMN IF NOT EXISTS emergency_contact text NULL,
    ADD CONSTRAINT users_not_own_manager CHECK (manager_id IS NULL OR manager_id <> id);

CREATE INDEX IF NOT EXISTS employees_manager_id_idx ON employees (manager_id);
CREATE INDEX IF NOT EXISTS employees_team_id_idx ON employees (team_id);
