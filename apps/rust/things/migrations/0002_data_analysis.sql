-- Data analysis state is independent from FSRS; no modification to flashcards tables.
CREATE TABLE IF NOT EXISTS data_analysis_sources (
  url TEXT PRIMARY KEY NOT NULL,
  title TEXT NOT NULL,
  content_hash TEXT NOT NULL,
  fetched_at_ms INTEGER NOT NULL,
  sections_json TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS data_analysis_groups (
  id TEXT PRIMARY KEY NOT NULL,
  source_url TEXT NOT NULL,
  section TEXT NOT NULL,
  topic TEXT NOT NULL,
  group_json TEXT NOT NULL,
  created_at_ms INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS data_analysis_groups_topic_idx ON data_analysis_groups(topic);
CREATE TABLE IF NOT EXISTS data_analysis_runs (
  id TEXT PRIMARY KEY NOT NULL,
  groups_json TEXT NOT NULL,
  answers_json TEXT NOT NULL,
  status TEXT NOT NULL CHECK(status IN ('ready','active','submitted')),
  started_at_ms INTEGER,
  ended_at_ms INTEGER,
  created_at_ms INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS data_analysis_results (
  run_id TEXT NOT NULL REFERENCES data_analysis_runs(id),
  question_no INTEGER NOT NULL,
  topic TEXT NOT NULL,
  skill TEXT NOT NULL,
  is_correct INTEGER NOT NULL CHECK(is_correct IN (0,1)),
  elapsed_ms INTEGER NOT NULL,
  PRIMARY KEY(run_id, question_no)
);
CREATE INDEX IF NOT EXISTS data_analysis_results_skill_idx ON data_analysis_results(skill);
CREATE INDEX IF NOT EXISTS data_analysis_results_topic_idx ON data_analysis_results(topic);
