-- Administrator daily learning goals. Weekly and 35-day targets derive from
-- these values in the portal, so overshoot on one day still counts.
CREATE TABLE study_goals (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    daily_questions INTEGER NOT NULL,
    daily_study_ms INTEGER NOT NULL
);
INSERT INTO study_goals(id, daily_questions, daily_study_ms) VALUES(1, 400, 43200000);
