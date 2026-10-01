CREATE EXTENSION IF NOT EXISTS postgis;

-- Anúncios normalizados. (source, external_id) garante que recoletar o mesmo
-- anúncio atualiza a linha em vez de duplicar.
CREATE TABLE listings (
    id                      UUID PRIMARY KEY,
    source                  TEXT NOT NULL,
    external_id             TEXT NOT NULL,
    title                   TEXT NOT NULL,
    transaction             TEXT NOT NULL,
    kind                    TEXT NOT NULL,
    price_brl               DOUBLE PRECISION,
    area_m2                 DOUBLE PRECISION,
    bedrooms                INTEGER,
    bathrooms               INTEGER,
    parking_spots           INTEGER,
    state                   TEXT,
    municipality            TEXT,
    municipality_ibge_code  TEXT,
    neighborhood            TEXT,
    neighborhood_slug       TEXT,
    postal_code             TEXT,
    geom                    GEOGRAPHY(POINT, 4326),
    source_url              TEXT,
    collected_at            TIMESTAMPTZ NOT NULL,
    updated_at              TIMESTAMPTZ NOT NULL,
    UNIQUE (source, external_id)
);

CREATE INDEX listings_municipality_idx ON listings (municipality_ibge_code, neighborhood_slug);
CREATE INDEX listings_geom_idx ON listings USING GIST (geom);

-- Regiões às quais indicadores se referem (município, bairro, setor
-- censitário, área de delegacia). geom é opcional até importarmos as malhas.
CREATE TABLE regions (
    id                      BIGSERIAL PRIMARY KEY,
    level                   TEXT NOT NULL,
    code                    TEXT NOT NULL,
    name                    TEXT NOT NULL,
    municipality_ibge_code  TEXT NOT NULL,
    geom                    GEOGRAPHY(MULTIPOLYGON, 4326),
    UNIQUE (level, code, municipality_ibge_code)
);

CREATE INDEX regions_geom_idx ON regions USING GIST (geom);

-- Valores de indicadores por região e período, sempre com procedência.
CREATE TABLE region_indicators (
    id              BIGSERIAL PRIMARY KEY,
    region_id       BIGINT NOT NULL REFERENCES regions (id) ON DELETE CASCADE,
    kind            TEXT NOT NULL,
    value           DOUBLE PRECISION NOT NULL,
    period_start    DATE NOT NULL,
    period_end      DATE NOT NULL,
    source          TEXT NOT NULL,
    source_url      TEXT,
    collected_at    TIMESTAMPTZ NOT NULL,
    UNIQUE (region_id, kind, period_start, period_end, source)
);
