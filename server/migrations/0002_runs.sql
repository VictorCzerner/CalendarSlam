CREATE TABLE IF NOT EXISTS runs (
    id BIGSERIAL PRIMARY KEY,
    nickname TEXT NOT NULL CHECK (char_length(nickname) BETWEEN 1 AND 32),
    overall INTEGER NOT NULL CHECK (overall BETWEEN 0 AND 99),
    slams_won INTEGER NOT NULL CHECK (slams_won BETWEEN 0 AND 4),
    attributes JSONB NOT NULL,
    sources JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS runs_leaderboard_idx
ON runs (slams_won DESC, overall DESC, created_at ASC);

