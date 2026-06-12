TRUNCATE TABLE edition_players, editions RESTART IDENTITY CASCADE;
TRUNCATE TABLE player_profiles;

INSERT INTO editions (level, tournament, year, surface, champion) VALUES
('ATP500', 'Washington', 2022, 'Hard', 'Nick Kyrgios'),
('GrandSlam', 'Australian Open', 2022, 'Hard', 'Rafael Nadal'),
('GrandSlam', 'Roland Garros', 2022, 'Clay', 'Rafael Nadal'),
('GrandSlam', 'Wimbledon', 2022, 'Grass', 'Novak Djokovic'),
('GrandSlam', 'US Open', 2022, 'Hard', 'Carlos Alcaraz'),
('ATP1000', 'Miami Masters', 2022, 'Hard', 'Carlos Alcaraz'),
('ATP1000', 'Monte Carlo Masters', 2022, 'Clay', 'Stefanos Tsitsipas'),
('ATP500', 'Halle', 2022, 'Grass', 'Hubert Hurkacz'),
('ATP500', 'Barcelona', 2022, 'Clay', 'Carlos Alcaraz'),
('ATP250', 'Adelaide 1', 2022, 'Hard', 'Gael Monfils');

INSERT INTO edition_players (edition_id, player_name, best_round)
SELECT e.id, p.player_name, p.best_round
FROM editions e
JOIN (VALUES
('Washington', 2022, 'Nick Kyrgios', 'W'),
('Washington', 2022, 'Yoshihito Nishioka', 'F'),
('Washington', 2022, 'Andrey Rublev', 'SF'),
('Washington', 2022, 'Mikael Ymer', 'F'),
('Washington', 2022, 'J J Wolf', 'QF'),
('Australian Open', 2022, 'Rafael Nadal', 'W'),
('Australian Open', 2022, 'Daniil Medvedev', 'F'),
('Australian Open', 2022, 'Stefanos Tsitsipas', 'SF'),
('Roland Garros', 2022, 'Rafael Nadal', 'W'),
('Roland Garros', 2022, 'Casper Ruud', 'F'),
('Roland Garros', 2022, 'Alexander Zverev', 'SF'),
('Wimbledon', 2022, 'Novak Djokovic', 'W'),
('Wimbledon', 2022, 'Nick Kyrgios', 'F'),
('Wimbledon', 2022, 'Cameron Norrie', 'SF'),
('US Open', 2022, 'Carlos Alcaraz', 'W'),
('US Open', 2022, 'Casper Ruud', 'F'),
('US Open', 2022, 'Frances Tiafoe', 'SF'),
('Miami Masters', 2022, 'Carlos Alcaraz', 'W'),
('Miami Masters', 2022, 'Casper Ruud', 'F'),
('Miami Masters', 2022, 'Hubert Hurkacz', 'SF'),
('Monte Carlo Masters', 2022, 'Stefanos Tsitsipas', 'W'),
('Monte Carlo Masters', 2022, 'Alejandro Davidovich Fokina', 'F'),
('Monte Carlo Masters', 2022, 'Grigor Dimitrov', 'SF'),
('Halle', 2022, 'Hubert Hurkacz', 'W'),
('Halle', 2022, 'Daniil Medvedev', 'F'),
('Halle', 2022, 'Nick Kyrgios', 'SF'),
('Barcelona', 2022, 'Carlos Alcaraz', 'W'),
('Barcelona', 2022, 'Pablo Carreno Busta', 'F'),
('Barcelona', 2022, 'Alex De Minaur', 'SF'),
('Adelaide 1', 2022, 'Gael Monfils', 'W'),
('Adelaide 1', 2022, 'Karen Khachanov', 'F'),
('Adelaide 1', 2022, 'Thanasi Kokkinakis', 'SF')
) AS p(tournament, year, player_name, best_round)
ON e.tournament = p.tournament AND e.year = p.year;

INSERT INTO player_profiles (
    player_name, serve, forehand, backhand, return_rating, movement, mental, net, stamina,
    trademark, trademark_floor
) VALUES
('Nick Kyrgios', 99, 94, 95, 97, 93, 97, 97, 88, 'serve', 90),
('Yoshihito Nishioka', 47, 74, 73, 77, 84, 69, 77, 70, NULL, NULL),
('Andrey Rublev', 85, 96, 94, 91, 95, 92, 89, 87, 'forehand', 90),
('Mikael Ymer', 48, 72, 72, 76, 83, 68, 76, 72, NULL, NULL),
('J J Wolf', 84, 80, 79, 82, 78, 76, 82, 72, NULL, NULL),
('Rafael Nadal', 94, 99, 99, 98, 99, 99, 97, 97, 'forehand', 90),
('Daniil Medvedev', 92, 95, 96, 94, 96, 97, 93, 93, 'movement', 90),
('Stefanos Tsitsipas', 91, 96, 93, 94, 94, 93, 94, 90, 'forehand', 90),
('Novak Djokovic', 91, 99, 99, 98, 99, 99, 98, 96, 'backhand', 90),
('Carlos Alcaraz', 90, 99, 97, 95, 99, 96, 96, 95, 'movement', 90),
('Casper Ruud', 84, 97, 95, 91, 97, 94, 90, 95, 'movement', 90),
('Frances Tiafoe', 90, 93, 91, 92, 94, 90, 92, 88, 'movement', 90),
('Hubert Hurkacz', 96, 91, 92, 95, 90, 92, 95, 86, 'serve', 90),
('Alejandro Davidovich Fokina', 79, 90, 89, 89, 94, 88, 88, 91, 'movement', 86),
('Grigor Dimitrov', 88, 94, 94, 96, 93, 91, 96, 88, 'backhand', 90),
('Pablo Carreno Busta', 82, 92, 93, 90, 93, 91, 89, 90, 'movement', 86),
('Alex De Minaur', 74, 91, 92, 91, 98, 93, 91, 94, 'movement', 86),
('Gael Monfils', 90, 91, 90, 91, 96, 90, 92, 90, 'movement', 90),
('Karen Khachanov', 92, 91, 89, 89, 88, 86, 88, 85, 'forehand', 86),
('Thanasi Kokkinakis', 91, 88, 86, 88, 84, 83, 88, 82, NULL, NULL),
('Alexander Zverev', 94, 95, 97, 93, 95, 93, 92, 92, 'serve', 90),
('Cameron Norrie', 78, 89, 90, 89, 94, 91, 88, 92, 'stamina', 90)
ON CONFLICT (player_name) DO UPDATE SET
    serve = EXCLUDED.serve,
    forehand = EXCLUDED.forehand,
    backhand = EXCLUDED.backhand,
    return_rating = EXCLUDED.return_rating,
    movement = EXCLUDED.movement,
    mental = EXCLUDED.mental,
    net = EXCLUDED.net,
    stamina = EXCLUDED.stamina,
    trademark = EXCLUDED.trademark,
    trademark_floor = EXCLUDED.trademark_floor,
    updated_at = now();
