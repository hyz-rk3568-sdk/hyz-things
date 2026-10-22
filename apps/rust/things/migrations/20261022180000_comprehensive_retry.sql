-- Keep each replay as an independent attempt without rewriting old scores.
-- Legacy archived runs are the first attempt by default.
ALTER TABLE data_analysis_runs ADD COLUMN origin_run_id TEXT;
ALTER TABLE data_analysis_runs ADD COLUMN attempt_no INTEGER NOT NULL DEFAULT 1;
CREATE UNIQUE INDEX IF NOT EXISTS idx_data_analysis_retries_origin_attempt
    ON data_analysis_runs(origin_run_id, attempt_no)
    WHERE origin_run_id IS NOT NULL;
