-- Company locations: offices, sites and remote hubs (Company > Locations).
-- A location holds no seats yet; the seat link comes with team later.
-- Not idempotent: applied once.

CREATE TABLE locations (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    name        text NOT NULL,
    kind        text NOT NULL CHECK (kind IN ('office', 'site', 'remote_hub')),
    address     text NOT NULL DEFAULT '',
    city        text NOT NULL DEFAULT '',
    country     text NOT NULL DEFAULT '',
    timezone    text NOT NULL DEFAULT '',
    note        text NOT NULL DEFAULT '',
    created_at  timestamptz NOT NULL DEFAULT now()
);
