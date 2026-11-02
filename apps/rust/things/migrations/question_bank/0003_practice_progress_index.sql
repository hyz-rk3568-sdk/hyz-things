-- Fast deduplicated progress lookups for original-paper and module drills.
-- Existing practice rows and saved answers remain unchanged.
CREATE INDEX IF NOT EXISTS idx_bank_practice_items_question_answer
  ON practice_items(question_id, answer);
