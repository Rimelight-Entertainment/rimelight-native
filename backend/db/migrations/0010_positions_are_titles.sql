-- Positions are titles, seats are slots (decision 0042, amends decision 0036).
-- A position is a job title (title, level, description, salary range). It has no team and no department.
-- A seat is one slot in one team holding one position and at most one person; it has no title,
-- level or salary of its own.
-- Not idempotent: applied once. Decision 0042.

-- 1. Salary range on positions, same rules as the seat salary columns had.
ALTER TABLE positions
    ADD COLUMN IF NOT EXISTS salary_min numeric(12, 2) NULL,
    ADD COLUMN IF NOT EXISTS salary_max numeric(12, 2) NULL,
    ADD COLUMN IF NOT EXISTS currency   char(3) NULL,
    ADD CONSTRAINT positions_salary_order CHECK (salary_min IS NULL OR salary_max IS NULL OR salary_min <= salary_max),
    ADD CONSTRAINT positions_salary_currency CHECK ((salary_min IS NULL AND salary_max IS NULL) OR currency IS NOT NULL);

-- 2a. Seats with no position: create one position per (title, level) that does not exist yet.
INSERT INTO positions (title, level)
SELECT DISTINCT s.title, s.level
FROM seats s
WHERE s.job_profile_id IS NULL
  AND NOT EXISTS (
        SELECT 1 FROM positions p WHERE p.title = s.title AND p.level = s.level
  );

-- 2b. Point those seats at the matching position (oldest match when there are several).
UPDATE seats s
SET job_profile_id = m.position_id
FROM (
    SELECT DISTINCT ON (title, level) title, level, id AS position_id
    FROM positions
    ORDER BY title, level, created_at, id
) m
WHERE s.job_profile_id IS NULL
  AND m.title = s.title
  AND m.level = s.level;

-- 2c. Each position takes the salary of one seat that uses it (a seat with a salary is preferred).
UPDATE positions p
SET salary_min = src.salary_min,
    salary_max = src.salary_max,
    currency   = src.currency
FROM (
    SELECT DISTINCT ON (job_profile_id)
           job_profile_id, salary_min, salary_max, currency
    FROM seats
    WHERE job_profile_id IS NOT NULL
    ORDER BY job_profile_id, (salary_max IS NULL), id
) src
WHERE src.job_profile_id = p.id
  AND src.salary_max IS NOT NULL;

-- 3. Seats become slots: drop their own title, level and salary, and require a position.
ALTER TABLE seats
    DROP CONSTRAINT IF EXISTS seats_salary_order,
    DROP CONSTRAINT IF EXISTS seats_salary_currency;

ALTER TABLE seats
    DROP COLUMN IF EXISTS title,
    DROP COLUMN IF EXISTS level,
    DROP COLUMN IF EXISTS salary_min,
    DROP COLUMN IF EXISTS salary_max,
    DROP COLUMN IF EXISTS currency;

ALTER TABLE seats
    ALTER COLUMN job_profile_id SET NOT NULL;

-- 4. Positions belong to no department.
ALTER TABLE positions
    DROP COLUMN IF EXISTS department_id;
