-- Photo captions are no longer collected or shown.
ALTER TABLE photos DROP COLUMN caption;

-- The location score moved to a gentler exponential curve (see scoring::location_score):
-- rescore the guesses already made so that every game is ranked with a single rule.
UPDATE guesses SET location_score = round((500 * exp(-distance_km / 1500))::numeric)::int;
