-- Enum-like values are defined and stored capitalised (Task, Bug, Normal, Active, ...) and shown as stored.
-- Existing lowercase rows are converted (matched case-insensitively), then the CHECK constraints
-- and the one-active-sprint index are replaced to accept the capitalised values.
-- Not idempotent: applied once.

ALTER TABLE issues DROP CONSTRAINT issues_type_check;
ALTER TABLE issues DROP CONSTRAINT issues_priority_check;
ALTER TABLE workflow_states DROP CONSTRAINT workflow_states_category_check;
ALTER TABLE sprints DROP CONSTRAINT sprints_state_check;
DROP INDEX sprints_one_active_idx;

UPDATE issues SET type = initcap(type)
    WHERE lower(type) IN ('task', 'bug', 'feature', 'epic');
UPDATE issues SET priority = initcap(priority)
    WHERE lower(priority) IN ('critical', 'high', 'normal', 'low');
UPDATE workflow_states SET category = initcap(category)
    WHERE lower(category) IN ('unstarted', 'started', 'done', 'cancelled');
UPDATE sprints SET state = initcap(state)
    WHERE lower(state) IN ('planned', 'active', 'closed');

ALTER TABLE issues ADD CONSTRAINT issues_type_check
    CHECK (type IN ('Task', 'Bug', 'Feature', 'Epic'));
ALTER TABLE issues ADD CONSTRAINT issues_priority_check
    CHECK (priority IN ('Critical', 'High', 'Normal', 'Low'));
ALTER TABLE workflow_states ADD CONSTRAINT workflow_states_category_check
    CHECK (category IN ('Unstarted', 'Started', 'Done', 'Cancelled'));
ALTER TABLE sprints ADD CONSTRAINT sprints_state_check
    CHECK (state IN ('Planned', 'Active', 'Closed'));

-- At most one active sprint per project.
CREATE UNIQUE INDEX sprints_one_active_idx ON sprints (project_id)
    WHERE state = 'Active' AND deleted_at IS NULL;
