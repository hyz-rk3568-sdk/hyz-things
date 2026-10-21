-- Pause state is persisted on the server: app reloads never advance a paused timer.
ALTER TABLE data_analysis_runs ADD COLUMN paused_at_ms INTEGER;
ALTER TABLE data_analysis_runs ADD COLUMN paused_total_ms INTEGER NOT NULL DEFAULT 0;
CREATE INDEX IF NOT EXISTS idx_data_analysis_runs_history
    ON data_analysis_runs(status, ended_at_ms DESC);
