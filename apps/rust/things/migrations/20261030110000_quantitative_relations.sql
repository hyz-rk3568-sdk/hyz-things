-- Quantitative relations does not share scheduling or results with data analysis or FSRS.
CREATE TABLE quantitative_relations_card_usage (
    card_id TEXT PRIMARY KEY NOT NULL,
    vcnt INTEGER NOT NULL CHECK(vcnt >= 0),
    selected_count INTEGER NOT NULL CHECK(selected_count >= 0)
);
CREATE TABLE quantitative_relations_jobs (
    id TEXT PRIMARY KEY NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('running','ready','failed')),
    verified INTEGER NOT NULL DEFAULT 0 CHECK(verified BETWEEN 0 AND 10),
    message TEXT NOT NULL DEFAULT '',
    run_id TEXT,
    created_at_ms INTEGER NOT NULL
);
CREATE UNIQUE INDEX quantitative_relations_one_running_job ON quantitative_relations_jobs(state) WHERE state='running';
CREATE TABLE quantitative_relations_runs (
    id TEXT PRIMARY KEY NOT NULL,
    questions_json TEXT NOT NULL,
    answers_json TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('prepared','active','submitted')),
    started_at_ms INTEGER,
    deadline_at_ms INTEGER,
    ended_at_ms INTEGER,
    created_at_ms INTEGER NOT NULL,
    origin_run_id TEXT REFERENCES quantitative_relations_runs(id)
);
CREATE TABLE quantitative_relations_results (
    run_id TEXT NOT NULL REFERENCES quantitative_relations_runs(id),
    question_no INTEGER NOT NULL CHECK(question_no BETWEEN 0 AND 9),
    card_id TEXT NOT NULL,
    was_answered INTEGER NOT NULL CHECK(was_answered IN (0,1)),
    is_correct INTEGER NOT NULL CHECK(is_correct IN (0,1)),
    PRIMARY KEY(run_id,question_no)
);
CREATE INDEX quantitative_relations_results_card ON quantitative_relations_results(card_id);
CREATE TABLE quantitative_relations_settings (
    id INTEGER PRIMARY KEY CHECK(id=1),
    user_prompt TEXT NOT NULL
);

-- Preserve server-authoritative paused time for quantitative relations; do not alter existing runs.
ALTER TABLE quantitative_relations_runs ADD COLUMN paused_at_ms INTEGER;
ALTER TABLE quantitative_relations_runs ADD COLUMN paused_total_ms INTEGER NOT NULL DEFAULT 0
    CHECK(paused_total_ms >= 0);
