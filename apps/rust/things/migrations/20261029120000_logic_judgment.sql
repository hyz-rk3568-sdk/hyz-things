-- Logic judgment does not share scheduling or results with data analysis or FSRS.
CREATE TABLE logic_judgment_card_usage (
    card_id TEXT PRIMARY KEY NOT NULL,
    vcnt INTEGER NOT NULL CHECK(vcnt >= 0),
    selected_count INTEGER NOT NULL CHECK(selected_count >= 0)
);
CREATE TABLE logic_judgment_jobs (
    id TEXT PRIMARY KEY NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('running','ready','failed')),
    verified INTEGER NOT NULL DEFAULT 0 CHECK(verified BETWEEN 0 AND 10),
    message TEXT NOT NULL DEFAULT '',
    run_id TEXT,
    created_at_ms INTEGER NOT NULL
);
CREATE UNIQUE INDEX logic_judgment_one_running_job ON logic_judgment_jobs(state) WHERE state='running';
CREATE TABLE logic_judgment_runs (
    id TEXT PRIMARY KEY NOT NULL,
    questions_json TEXT NOT NULL,
    answers_json TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('prepared','active','submitted')),
    started_at_ms INTEGER,
    deadline_at_ms INTEGER,
    ended_at_ms INTEGER,
    created_at_ms INTEGER NOT NULL,
    origin_run_id TEXT REFERENCES logic_judgment_runs(id)
);
CREATE TABLE logic_judgment_results (
    run_id TEXT NOT NULL REFERENCES logic_judgment_runs(id),
    question_no INTEGER NOT NULL CHECK(question_no BETWEEN 0 AND 9),
    card_id TEXT NOT NULL,
    was_answered INTEGER NOT NULL CHECK(was_answered IN (0,1)),
    is_correct INTEGER NOT NULL CHECK(is_correct IN (0,1)),
    PRIMARY KEY(run_id,question_no)
);
CREATE INDEX logic_judgment_results_card ON logic_judgment_results(card_id);
CREATE TABLE logic_judgment_settings (
    id INTEGER PRIMARY KEY CHECK(id=1),
    user_prompt TEXT NOT NULL
);
