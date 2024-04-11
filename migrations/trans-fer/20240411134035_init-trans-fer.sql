-- Add migration script here
CREATE TABLE IF NOT EXISTS autoconversions (asset_from TEXT UNIQUE NOT NULL, asset_to TEXT NOT NULL, scale UNSIGNED INT NOT NULL);
CREATE UNIQUE INDEX autoconversions_idx ON autoconversions(asset_from);
