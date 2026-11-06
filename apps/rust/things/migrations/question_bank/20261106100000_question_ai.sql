-- Session-scoped AI history cannot be safely associated with a stem fingerprint.
DROP TABLE question_ai_messages;
DROP TABLE question_ai_reviews;

CREATE TABLE question_ai_reviews (
    question_id TEXT NOT NULL,
    stem_fingerprint TEXT NOT NULL,
    thread_id TEXT,
    status TEXT NOT NULL DEFAULT 'idle' CHECK(status IN ('idle','running','complete','stopped','failed')),
    error TEXT,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY(question_id, stem_fingerprint)
);
CREATE TABLE question_ai_messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    question_id TEXT NOT NULL,
    stem_fingerprint TEXT NOT NULL,
    role TEXT NOT NULL CHECK(role IN ('user','assistant')),
    content TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    FOREIGN KEY(question_id, stem_fingerprint)
        REFERENCES question_ai_reviews(question_id, stem_fingerprint) ON DELETE CASCADE
);
CREATE INDEX idx_question_ai_messages_review ON question_ai_messages(question_id,stem_fingerprint,id);

CREATE TRIGGER data_revision_question_ai_reviews_insert AFTER INSERT ON question_ai_reviews BEGIN
    UPDATE data_revision SET revision = revision + 1 WHERE id = 1;
END;
CREATE TRIGGER data_revision_question_ai_reviews_update AFTER UPDATE ON question_ai_reviews BEGIN
    UPDATE data_revision SET revision = revision + 1 WHERE id = 1;
END;
CREATE TRIGGER data_revision_question_ai_reviews_delete AFTER DELETE ON question_ai_reviews BEGIN
    UPDATE data_revision SET revision = revision + 1 WHERE id = 1;
END;
CREATE TRIGGER data_revision_question_ai_messages_insert AFTER INSERT ON question_ai_messages BEGIN
    UPDATE data_revision SET revision = revision + 1 WHERE id = 1;
END;
CREATE TRIGGER data_revision_question_ai_messages_update AFTER UPDATE ON question_ai_messages BEGIN
    UPDATE data_revision SET revision = revision + 1 WHERE id = 1;
END;
CREATE TRIGGER data_revision_question_ai_messages_delete AFTER DELETE ON question_ai_messages BEGIN
    UPDATE data_revision SET revision = revision + 1 WHERE id = 1;
END;
UPDATE data_revision SET revision = revision + 1 WHERE id = 1;
