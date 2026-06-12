ALTER TABLE player_profiles
ADD COLUMN IF NOT EXISTS trademark TEXT,
ADD COLUMN IF NOT EXISTS trademark_floor SMALLINT CHECK (
    trademark_floor IS NULL OR trademark_floor BETWEEN 0 AND 99
);
