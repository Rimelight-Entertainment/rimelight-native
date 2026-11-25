-- Canvas storage limits are fixed in code (1 GB per person, 10 GB per project; owner, 2026-11-19). The per-subject
-- override table and its admin routes are gone, so the table is dropped. It held no rows that the code still reads.
DROP TABLE IF EXISTS storage_quotas;
