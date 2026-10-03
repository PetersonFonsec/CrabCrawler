-- Módulo de segurança pública.
--
-- Estatísticas criminais oficiais por região e período. Cada linha guarda o
-- rótulo original da fonte e a procedência completa; taxas por 100 mil não são
-- gravadas: são calculadas na leitura com a população (region_indicators) e a
-- população usada é devolvida junto.
CREATE TABLE crime_statistics (
    id                  BIGSERIAL PRIMARY KEY,
    region_id           BIGINT NOT NULL REFERENCES regions (id) ON DELETE CASCADE,
    crime_type          TEXT NOT NULL,
    counting_unit       TEXT NOT NULL CHECK (counting_unit IN ('occurrences', 'victims')),
    count               BIGINT NOT NULL CHECK (count >= 0),
    period_start        DATE NOT NULL,
    period_end          DATE NOT NULL,
    period_granularity  TEXT NOT NULL CHECK (period_granularity IN ('month', 'year')),
    source              TEXT NOT NULL,
    source_label        TEXT NOT NULL,
    source_url          TEXT,
    dataset_version     TEXT,
    collected_at        TIMESTAMPTZ NOT NULL,
    CHECK (period_end >= period_start),
    UNIQUE (region_id, crime_type, counting_unit, period_start, period_end, source, source_label)
);

CREATE INDEX crime_statistics_lookup_idx
    ON crime_statistics (region_id, crime_type, period_start);

-- Ocorrências individuais georreferenciadas (microdados de boletins).
-- Ainda sem importador: preparada para consultas por raio (ST_DWithin) quando
-- houver dataset com coordenadas validadas.
CREATE TABLE crime_occurrences (
    id                      BIGSERIAL PRIMARY KEY,
    source                  TEXT NOT NULL,
    external_id             TEXT NOT NULL,
    crime_type              TEXT NOT NULL,
    source_label            TEXT NOT NULL,
    occurred_on             DATE NOT NULL,
    geom                    GEOGRAPHY(POINT, 4326),
    municipality_ibge_code  TEXT,
    neighborhood            TEXT,
    police_unit             TEXT,
    source_url              TEXT,
    dataset_version         TEXT,
    collected_at            TIMESTAMPTZ NOT NULL,
    UNIQUE (source, external_id)
);

CREATE INDEX crime_occurrences_geom_idx ON crime_occurrences USING GIST (geom);
CREATE INDEX crime_occurrences_type_date_idx ON crime_occurrences (crime_type, occurred_on);

-- Registro de cada importação: permite responder "de qual arquivo veio" e
-- "quando foi a última atualização" mesmo quando nada novo foi gravado.
CREATE TABLE security_dataset_imports (
    id                  BIGSERIAL PRIMARY KEY,
    source              TEXT NOT NULL,
    source_url          TEXT,
    dataset_version     TEXT,
    scope               TEXT NOT NULL,
    records_read        INTEGER NOT NULL,
    records_stored      INTEGER NOT NULL,
    records_skipped     INTEGER NOT NULL,
    report              JSONB NOT NULL DEFAULT '{}'::jsonb,
    imported_at         TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX security_dataset_imports_source_idx
    ON security_dataset_imports (source, imported_at DESC);
