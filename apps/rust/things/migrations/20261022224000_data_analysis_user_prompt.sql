-- One shared editable generation prompt for the single-user study portal.
-- The fixed server policy and dynamic classification plan are never stored here.
CREATE TABLE IF NOT EXISTS data_analysis_prompt_settings (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    user_prompt TEXT NOT NULL,
    updated_at_ms INTEGER NOT NULL
);
