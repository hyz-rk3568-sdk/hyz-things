-- Manual pause is retired. The clock runs only while a drill is active and is
-- closed when the paper is paused or the learner leaves the answering screen.
CREATE TABLE study_time_state_next (
  id INTEGER PRIMARY KEY NOT NULL CHECK(id = 1),
  status TEXT NOT NULL DEFAULT 'idle' CHECK(status IN ('idle','running')),
  module TEXT NOT NULL DEFAULT '',
  phase TEXT NOT NULL DEFAULT 'answer',
  started_at_ms INTEGER,
  last_seen_at_ms INTEGER,
  revision INTEGER NOT NULL DEFAULT 0
);
INSERT INTO study_time_state_next(id,status,module,phase,started_at_ms,last_seen_at_ms,revision)
SELECT id,
       CASE WHEN status = 'running' THEN 'running' ELSE 'idle' END,
       module,
       phase,
       CASE WHEN status = 'running' THEN started_at_ms ELSE NULL END,
       CASE WHEN status = 'running' THEN last_seen_at_ms ELSE NULL END,
       revision
FROM study_time_state;
DROP TABLE study_time_state;
ALTER TABLE study_time_state_next RENAME TO study_time_state;
