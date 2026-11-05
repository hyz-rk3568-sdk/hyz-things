ALTER TABLE questions ADD COLUMN content_fingerprint TEXT;
CREATE INDEX idx_bank_questions_content_fingerprint ON questions(content_fingerprint);
