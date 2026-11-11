ALTER TABLE question_ai_requests ADD COLUMN assistant_message_id INTEGER;

CREATE TABLE question_ai_events (
    question_id TEXT NOT NULL,
    stem_fingerprint TEXT NOT NULL,
    run_id TEXT NOT NULL REFERENCES question_ai_requests(run_id) ON DELETE CASCADE,
    seq INTEGER NOT NULL,
    kind TEXT NOT NULL,
    timestamp_ms INTEGER NOT NULL,
    payload TEXT NOT NULL,
    PRIMARY KEY(run_id, seq),
    FOREIGN KEY(question_id, stem_fingerprint)
        REFERENCES question_ai_reviews(question_id, stem_fingerprint) ON DELETE CASCADE
);
CREATE INDEX idx_question_ai_events_identity ON question_ai_events(question_id, stem_fingerprint, run_id, seq);
