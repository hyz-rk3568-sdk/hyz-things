-- One active study session per portal. The owner is the administrator session
-- (this device has a single administrator account).
CREATE TABLE study_time_state (
  id INTEGER PRIMARY KEY NOT NULL CHECK(id = 1),
  status TEXT NOT NULL DEFAULT 'idle' CHECK(status IN ('idle','running','paused')),
  module TEXT NOT NULL DEFAULT '',
  phase TEXT NOT NULL DEFAULT 'answer',
  started_at_ms INTEGER,
  last_seen_at_ms INTEGER,
  manual_paused INTEGER NOT NULL DEFAULT 0 CHECK(manual_paused IN (0,1)),
  revision INTEGER NOT NULL DEFAULT 0
);
INSERT INTO study_time_state(id) VALUES(1);
CREATE TABLE study_time_segments (
  id INTEGER PRIMARY KEY,
  module TEXT NOT NULL,
  phase TEXT NOT NULL,
  started_at_ms INTEGER NOT NULL,
  ended_at_ms INTEGER NOT NULL,
  effective_ms INTEGER NOT NULL CHECK(effective_ms >= 0)
);
CREATE INDEX study_time_segments_start ON study_time_segments(started_at_ms);
CREATE TABLE study_time_events (
  event_id TEXT PRIMARY KEY NOT NULL,
  created_at_ms INTEGER NOT NULL
);
