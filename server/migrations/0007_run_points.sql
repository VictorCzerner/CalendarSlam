-- Total ATP points across the four slams becomes the leaderboard's primary metric.
ALTER TABLE runs ADD COLUMN IF NOT EXISTS points INTEGER NOT NULL DEFAULT 0;

CREATE INDEX IF NOT EXISTS runs_points_idx
ON runs (points DESC, overall DESC, created_at ASC);
