-- Seeds the first employee so that someone can sign in (decision 0014).
-- Users are otherwise pre-provisioned by an admin.
--
-- Run with psql, passing the owner's email:
--   psql "$DATABASE_URL" -v owner_email='owner@example.com' -f 0002_seed_owner.sql
-- psql fails if owner_email is not set. The insert runs only when users is empty,
-- so re-running this file later does nothing.

INSERT INTO employees (id, email, name)
SELECT gen_random_uuid(), :'owner_email', split_part(:'owner_email', '@', 1)
WHERE NOT EXISTS (SELECT 1 FROM employees);
