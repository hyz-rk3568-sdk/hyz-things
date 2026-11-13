-- Preserve unknown remote execution history while requiring explicit replay acknowledgement.
ALTER TABLE shenlun_jobs ADD COLUMN acknowledged_at_ms INTEGER;
