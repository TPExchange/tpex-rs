-- Add migration script here
UPDATE tokens
SET user = replace(user, "/", ".")
