-- Server-side resume position and one active editor lease per unfinished paper.
-- Tokens are random server-generated UUIDs and never appear in GET run/history.
ALTER TABLE data_analysis_runs ADD COLUMN current_question INTEGER NOT NULL DEFAULT 0;
ALTER TABLE data_analysis_runs ADD COLUMN controller_token TEXT;
CREATE INDEX IF NOT EXISTS idx_data_analysis_unfinished
    ON data_analysis_runs(status, created_at_ms DESC);
