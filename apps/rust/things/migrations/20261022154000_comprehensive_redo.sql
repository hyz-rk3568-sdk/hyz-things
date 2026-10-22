-- Redo creates a separate historical attempt without modifying its source.
-- An original paper has attempt_number=1 and no original_run_id.
ALTER TABLE data_analysis_runs ADD COLUMN original_run_id TEXT REFERENCES data_analysis_runs(id);
ALTER TABLE data_analysis_runs ADD COLUMN attempt_number INTEGER NOT NULL DEFAULT 1;
CREATE INDEX IF NOT EXISTS idx_data_analysis_runs_original_attempt
  ON data_analysis_runs(original_run_id, attempt_number);
