-- Optional reference price per alias: the stored price row used to bill requests
-- through this prefix. Null falls back to the connection pin, then the model id.

ALTER TABLE aliases ADD COLUMN pricing_model TEXT;
