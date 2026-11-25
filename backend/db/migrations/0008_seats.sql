-- Positions as seats (decision 0036, SP.5).
-- A seat is one place in one team, holding at most one person. Job profiles stay in the
-- `positions` table (renamed in the UI only, to keep the existing column and routes).
-- Not idempotent: the dev server applies it only when the seats table is missing.
-- Money is visible to admin and HR only; that is enforced in the API, not in SQL.

CREATE TABLE IF NOT EXISTS seats (
    id                uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    team_id           uuid NOT NULL REFERENCES teams (id),
    job_profile_id    uuid NULL REFERENCES positions (id),
    title             text NOT NULL,
    level             text NOT NULL DEFAULT '',
    state             text NOT NULL DEFAULT 'planned'
                      CHECK (state IN ('planned', 'open', 'filled', 'closed')),
    person_id         uuid NULL REFERENCES employees (id),
    reports_to_seat_id uuid NULL REFERENCES seats (id),
    salary_min        numeric(12, 2) NULL,
    salary_max        numeric(12, 2) NULL,
    currency          char(3) NULL,
    created_at        timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT seats_not_own_parent CHECK (reports_to_seat_id IS NULL OR reports_to_seat_id <> id),
    CONSTRAINT seats_filled_has_person CHECK ((state = 'filled') = (person_id IS NOT NULL)),
    CONSTRAINT seats_closed_is_empty CHECK (state <> 'closed' OR person_id IS NULL),
    CONSTRAINT seats_salary_order CHECK (salary_min IS NULL OR salary_max IS NULL OR salary_min <= salary_max),
    CONSTRAINT seats_salary_currency CHECK ((salary_min IS NULL AND salary_max IS NULL) OR currency IS NOT NULL)
);

-- One seat per person at a time.
CREATE UNIQUE INDEX IF NOT EXISTS seats_person_id_key ON seats (person_id) WHERE person_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS seats_team_id_idx ON seats (team_id);
CREATE INDEX IF NOT EXISTS seats_reports_to_idx ON seats (reports_to_seat_id);

-- Yearly budget cap per team (admin and HR only). Used and remaining are computed from the seats.
ALTER TABLE teams
    ADD COLUMN IF NOT EXISTS budget_cap      numeric(14, 2) NULL,
    ADD COLUMN IF NOT EXISTS budget_currency char(3) NULL,
    ADD CONSTRAINT teams_budget_currency CHECK (budget_cap IS NULL OR budget_currency IS NOT NULL);
