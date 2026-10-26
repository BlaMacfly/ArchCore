-- Bilan des combats de guildes : victoires, nuls, défaites et points (score cumulé des combats).
ALTER TABLE guild
    ADD COLUMN IF NOT EXISTS win     INT NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS draw    INT NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS lose    INT NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS points  INT NOT NULL DEFAULT 0;
