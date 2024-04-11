-- Add migration script here
CREATE TABLE IF NOT EXISTS tokens (token BINARY(16) UNIQUE NOT NULL, level INT NOT NULL, user TEXT NOT NULL);
CREATE UNIQUE INDEX tokens_idx ON tokens(token);