-- Shared exposure identity across comprehensive and number-locating practice.
-- Keys are conservative official source+section identities. Never treat a
-- rephrased paragraph of the same official chapter as a new unseen article.
CREATE TABLE IF NOT EXISTS data_analysis_material_seen (
    material_key TEXT PRIMARY KEY NOT NULL,
    source_url TEXT NOT NULL,
    section TEXT NOT NULL,
    first_mode TEXT NOT NULL CHECK(first_mode IN ('comprehensive','locating')),
    seen_at_ms INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_data_analysis_material_seen_source
    ON data_analysis_material_seen(source_url, section);

-- Reuse each validated five-question locating set without invoking AI again.
CREATE TABLE IF NOT EXISTS data_analysis_locating_sets (
    group_id TEXT PRIMARY KEY NOT NULL REFERENCES data_analysis_groups(id),
    questions_json TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL
);

-- Locating attempts are independent of multiple-choice results and FSRS.
CREATE TABLE IF NOT EXISTS data_analysis_locating_runs (
    id TEXT PRIMARY KEY NOT NULL,
    group_id TEXT NOT NULL REFERENCES data_analysis_groups(id),
    material_key TEXT NOT NULL,
    questions_json TEXT NOT NULL,
    answers_json TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('ready','active','complete')),
    created_at_ms INTEGER NOT NULL,
    started_at_ms INTEGER
);
CREATE INDEX IF NOT EXISTS idx_data_analysis_locating_runs_material
    ON data_analysis_locating_runs(material_key, created_at_ms);
