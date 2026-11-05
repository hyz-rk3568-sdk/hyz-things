CREATE TABLE data_revision (id INTEGER PRIMARY KEY CHECK (id = 1), revision INTEGER NOT NULL);
INSERT INTO data_revision(id, revision) VALUES (1, 0);

CREATE TRIGGER data_revision_papers_insert AFTER INSERT ON papers
BEGIN
    UPDATE data_revision SET revision = revision + 1 WHERE id = 1;
END;

CREATE TRIGGER data_revision_papers_update AFTER UPDATE ON papers
BEGIN
    UPDATE data_revision SET revision = revision + 1 WHERE id = 1;
END;

CREATE TRIGGER data_revision_papers_delete AFTER DELETE ON papers
BEGIN
    UPDATE data_revision SET revision = revision + 1 WHERE id = 1;
END;

CREATE TRIGGER data_revision_identities_insert AFTER INSERT ON identities
BEGIN
    UPDATE data_revision SET revision = revision + 1 WHERE id = 1;
END;

CREATE TRIGGER data_revision_identities_update AFTER UPDATE ON identities
BEGIN
    UPDATE data_revision SET revision = revision + 1 WHERE id = 1;
END;

CREATE TRIGGER data_revision_identities_delete AFTER DELETE ON identities
BEGIN
    UPDATE data_revision SET revision = revision + 1 WHERE id = 1;
END;

CREATE TRIGGER data_revision_questions_insert AFTER INSERT ON questions
BEGIN
    UPDATE data_revision SET revision = revision + 1 WHERE id = 1;
END;

CREATE TRIGGER data_revision_questions_update AFTER UPDATE ON questions
BEGIN
    UPDATE data_revision SET revision = revision + 1 WHERE id = 1;
END;

CREATE TRIGGER data_revision_questions_delete AFTER DELETE ON questions
BEGIN
    UPDATE data_revision SET revision = revision + 1 WHERE id = 1;
END;

CREATE TRIGGER data_revision_paper_materials_insert AFTER INSERT ON paper_materials
BEGIN
    UPDATE data_revision SET revision = revision + 1 WHERE id = 1;
END;

CREATE TRIGGER data_revision_paper_materials_update AFTER UPDATE ON paper_materials
BEGIN
    UPDATE data_revision SET revision = revision + 1 WHERE id = 1;
END;

CREATE TRIGGER data_revision_paper_materials_delete AFTER DELETE ON paper_materials
BEGIN
    UPDATE data_revision SET revision = revision + 1 WHERE id = 1;
END;

CREATE TRIGGER data_revision_practice_sessions_insert AFTER INSERT ON practice_sessions
BEGIN
    UPDATE data_revision SET revision = revision + 1 WHERE id = 1;
END;

CREATE TRIGGER data_revision_practice_sessions_update AFTER UPDATE ON practice_sessions
BEGIN
    UPDATE data_revision SET revision = revision + 1 WHERE id = 1;
END;

CREATE TRIGGER data_revision_practice_sessions_delete AFTER DELETE ON practice_sessions
BEGIN
    UPDATE data_revision SET revision = revision + 1 WHERE id = 1;
END;

CREATE TRIGGER data_revision_practice_items_insert AFTER INSERT ON practice_items
BEGIN
    UPDATE data_revision SET revision = revision + 1 WHERE id = 1;
END;

CREATE TRIGGER data_revision_practice_items_update AFTER UPDATE ON practice_items
BEGIN
    UPDATE data_revision SET revision = revision + 1 WHERE id = 1;
END;

CREATE TRIGGER data_revision_practice_items_delete AFTER DELETE ON practice_items
BEGIN
    UPDATE data_revision SET revision = revision + 1 WHERE id = 1;
END;

CREATE TRIGGER data_revision_practice_attempts_insert AFTER INSERT ON practice_attempts
BEGIN
    UPDATE data_revision SET revision = revision + 1 WHERE id = 1;
END;

CREATE TRIGGER data_revision_practice_attempts_update AFTER UPDATE ON practice_attempts
BEGIN
    UPDATE data_revision SET revision = revision + 1 WHERE id = 1;
END;

CREATE TRIGGER data_revision_practice_attempts_delete AFTER DELETE ON practice_attempts
BEGIN
    UPDATE data_revision SET revision = revision + 1 WHERE id = 1;
END;

CREATE TRIGGER data_revision_question_progress_insert AFTER INSERT ON question_progress
BEGIN
    UPDATE data_revision SET revision = revision + 1 WHERE id = 1;
END;

CREATE TRIGGER data_revision_question_progress_update AFTER UPDATE ON question_progress
BEGIN
    UPDATE data_revision SET revision = revision + 1 WHERE id = 1;
END;

CREATE TRIGGER data_revision_question_progress_delete AFTER DELETE ON question_progress
BEGIN
    UPDATE data_revision SET revision = revision + 1 WHERE id = 1;
END;

CREATE TRIGGER data_revision_favorites_insert AFTER INSERT ON favorites
BEGIN
    UPDATE data_revision SET revision = revision + 1 WHERE id = 1;
END;

CREATE TRIGGER data_revision_favorites_update AFTER UPDATE ON favorites
BEGIN
    UPDATE data_revision SET revision = revision + 1 WHERE id = 1;
END;

CREATE TRIGGER data_revision_favorites_delete AFTER DELETE ON favorites
BEGIN
    UPDATE data_revision SET revision = revision + 1 WHERE id = 1;
END;

CREATE TRIGGER data_revision_module_practice_settings_insert AFTER INSERT ON module_practice_settings
BEGIN
    UPDATE data_revision SET revision = revision + 1 WHERE id = 1;
END;

CREATE TRIGGER data_revision_module_practice_settings_update AFTER UPDATE ON module_practice_settings
BEGIN
    UPDATE data_revision SET revision = revision + 1 WHERE id = 1;
END;

CREATE TRIGGER data_revision_module_practice_settings_delete AFTER DELETE ON module_practice_settings
BEGIN
    UPDATE data_revision SET revision = revision + 1 WHERE id = 1;
END;

CREATE TRIGGER data_revision_study_plan_settings_insert AFTER INSERT ON study_plan_settings
BEGIN
    UPDATE data_revision SET revision = revision + 1 WHERE id = 1;
END;

CREATE TRIGGER data_revision_study_plan_settings_update AFTER UPDATE ON study_plan_settings
BEGIN
    UPDATE data_revision SET revision = revision + 1 WHERE id = 1;
END;

CREATE TRIGGER data_revision_study_plan_settings_delete AFTER DELETE ON study_plan_settings
BEGIN
    UPDATE data_revision SET revision = revision + 1 WHERE id = 1;
END;

CREATE TRIGGER data_revision_study_plan_items_insert AFTER INSERT ON study_plan_items
BEGIN
    UPDATE data_revision SET revision = revision + 1 WHERE id = 1;
END;

CREATE TRIGGER data_revision_study_plan_items_update AFTER UPDATE ON study_plan_items
BEGIN
    UPDATE data_revision SET revision = revision + 1 WHERE id = 1;
END;

CREATE TRIGGER data_revision_study_plan_items_delete AFTER DELETE ON study_plan_items
BEGIN
    UPDATE data_revision SET revision = revision + 1 WHERE id = 1;
END;

CREATE TRIGGER data_revision_question_ai_reviews_insert AFTER INSERT ON question_ai_reviews
BEGIN
    UPDATE data_revision SET revision = revision + 1 WHERE id = 1;
END;

CREATE TRIGGER data_revision_question_ai_reviews_update AFTER UPDATE ON question_ai_reviews
BEGIN
    UPDATE data_revision SET revision = revision + 1 WHERE id = 1;
END;

CREATE TRIGGER data_revision_question_ai_reviews_delete AFTER DELETE ON question_ai_reviews
BEGIN
    UPDATE data_revision SET revision = revision + 1 WHERE id = 1;
END;

CREATE TRIGGER data_revision_question_ai_messages_insert AFTER INSERT ON question_ai_messages
BEGIN
    UPDATE data_revision SET revision = revision + 1 WHERE id = 1;
END;

CREATE TRIGGER data_revision_question_ai_messages_update AFTER UPDATE ON question_ai_messages
BEGIN
    UPDATE data_revision SET revision = revision + 1 WHERE id = 1;
END;

CREATE TRIGGER data_revision_question_ai_messages_delete AFTER DELETE ON question_ai_messages
BEGIN
    UPDATE data_revision SET revision = revision + 1 WHERE id = 1;
END;
