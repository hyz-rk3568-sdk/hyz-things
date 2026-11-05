CREATE TABLE study_plan_settings (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    prediction_enabled INTEGER NOT NULL DEFAULT 1
);
INSERT INTO study_plan_settings(id, prediction_enabled) VALUES(1, 1);

-- Empty category means the whole primary module.
CREATE TABLE study_plan_items (
    module TEXT NOT NULL,
    category TEXT NOT NULL DEFAULT '',
    PRIMARY KEY(module, category)
);
