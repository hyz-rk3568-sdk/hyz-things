-- Preserve server-authoritative paused time for logic judgment; do not alter existing runs.
ALTER TABLE logic_judgment_runs ADD COLUMN paused_at_ms INTEGER;
ALTER TABLE logic_judgment_runs ADD COLUMN paused_total_ms INTEGER NOT NULL DEFAULT 0
    CHECK(paused_total_ms >= 0);
