CREATE TABLE IF NOT EXISTS editions (
    id BIGSERIAL PRIMARY KEY,
    level TEXT NOT NULL CHECK (level IN ('ATP250', 'ATP500', 'ATP1000', 'GrandSlam')),
    tournament TEXT NOT NULL,
    year INTEGER NOT NULL CHECK (year BETWEEN 1877 AND 2100),
    surface TEXT NOT NULL CHECK (surface IN ('Hard', 'Clay', 'Grass', 'Carpet')),
    champion TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT editions_level_tournament_year_key UNIQUE (level, tournament, year)
);

CREATE TABLE IF NOT EXISTS edition_players (
    edition_id BIGINT NOT NULL REFERENCES editions(id) ON DELETE CASCADE,
    player_name TEXT NOT NULL,
    best_round TEXT NOT NULL,
    PRIMARY KEY (edition_id, player_name)
);

CREATE INDEX IF NOT EXISTS editions_level_idx ON editions(level);
CREATE INDEX IF NOT EXISTS editions_level_tournament_idx ON editions(level, tournament);
CREATE INDEX IF NOT EXISTS edition_players_player_name_idx ON edition_players(player_name);
