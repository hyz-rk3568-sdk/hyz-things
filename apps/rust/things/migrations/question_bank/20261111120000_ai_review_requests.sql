CREATE TABLE question_ai_requests (
    question_id TEXT NOT NULL,
    stem_fingerprint TEXT NOT NULL,
    request_id TEXT NOT NULL,
    payload_hash TEXT NOT NULL,
    run_id TEXT NOT NULL UNIQUE,
    intent TEXT NOT NULL CHECK(intent IN ('initial','follow_up')),
    input_text TEXT,
    attachments TEXT NOT NULL,
    user_message_id INTEGER,
    retry_of TEXT,
    status TEXT NOT NULL CHECK(status IN ('running','complete','failed','stopped','unknown')),
    error TEXT,
    remote_uncertain INTEGER NOT NULL DEFAULT 0 CHECK(remote_uncertain IN (0,1)),
    new_thread_requested INTEGER NOT NULL DEFAULT 0 CHECK(new_thread_requested IN (0,1)),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY(question_id,stem_fingerprint,request_id),
    FOREIGN KEY(question_id,stem_fingerprint)
        REFERENCES question_ai_reviews(question_id,stem_fingerprint) ON DELETE CASCADE
);
CREATE INDEX idx_question_ai_requests_review ON question_ai_requests(question_id,stem_fingerprint,created_at);
CREATE UNIQUE INDEX idx_question_ai_requests_one_running
    ON question_ai_requests(question_id,stem_fingerprint) WHERE status='running';
