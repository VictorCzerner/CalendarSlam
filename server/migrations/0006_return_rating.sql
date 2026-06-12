-- "Slice" became "Devolucao"/Return: rename the column (Postgres carries the CHECK
-- constraint over to the new name automatically). Column is `return_rating` because
-- `return` is a reserved SQL keyword.
ALTER TABLE player_profiles RENAME COLUMN slice TO return_rating;
