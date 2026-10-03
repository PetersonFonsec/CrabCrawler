-- Regional Intelligence: perfil da região (setores censitários), serviços
-- próximos e áreas de risco ambiental.
--
-- Rastreabilidade: todo registro aponta para regional_datasets (fonte,
-- dataset, versão, URL, data de referência, coleta, granularidade) e guarda
-- o identificador original da fonte. regional_dataset_coverage diz quais
-- municípios cada dataset cobre: é o que separa "sem dado" de "a fonte foi
-- consultada e não há nada mapeado".

CREATE TABLE regional_datasets (
    id                      BIGSERIAL PRIMARY KEY,
    source                  TEXT NOT NULL,
    dataset_name            TEXT NOT NULL,
    dataset_version         TEXT NOT NULL,
    capability              TEXT NOT NULL,
    -- Categorias de serviço ou tipos de risco presentes neste dataset.
    categories              TEXT[] NOT NULL DEFAULT '{}',
    source_url              TEXT,
    reference_date          DATE,
    geographic_granularity  TEXT NOT NULL,
    collected_at            TIMESTAMPTZ NOT NULL,
    UNIQUE (source, dataset_name, dataset_version)
);

CREATE TABLE regional_dataset_coverage (
    dataset_id              BIGINT NOT NULL REFERENCES regional_datasets (id) ON DELETE CASCADE,
    municipality_ibge_code  TEXT NOT NULL,
    PRIMARY KEY (dataset_id, municipality_ibge_code)
);

CREATE INDEX regional_dataset_coverage_municipality_idx
    ON regional_dataset_coverage (municipality_ibge_code);

-- Cada execução de importação, inclusive as que falharam.
CREATE TABLE regional_imports (
    id                  BIGSERIAL PRIMARY KEY,
    source              TEXT NOT NULL,
    dataset_name        TEXT NOT NULL,
    dataset_version     TEXT,
    dataset_id          BIGINT REFERENCES regional_datasets (id) ON DELETE SET NULL,
    scope               TEXT NOT NULL,
    status              TEXT NOT NULL CHECK (status IN ('running', 'succeeded', 'failed')),
    records_read        INTEGER NOT NULL DEFAULT 0,
    records_stored      INTEGER NOT NULL DEFAULT 0,
    records_skipped     INTEGER NOT NULL DEFAULT 0,
    records_removed     INTEGER NOT NULL DEFAULT 0,
    error               TEXT,
    report              JSONB NOT NULL DEFAULT '{}'::jsonb,
    started_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at         TIMESTAMPTZ
);

CREATE INDEX regional_imports_source_idx
    ON regional_imports (source, dataset_name, started_at DESC);

-- Malha de setores censitários. GEOMETRY (não GEOGRAPHY) porque a consulta
-- principal é ponto-em-polígono (ST_Covers), mais simples e rápida em 4326.
CREATE TABLE census_sectors (
    code                    TEXT PRIMARY KEY CHECK (code ~ '^[0-9]{15}$'),
    municipality_ibge_code  TEXT NOT NULL,
    municipality_name       TEXT,
    area_km2                DOUBLE PRECISION,
    geom                    GEOMETRY(MULTIPOLYGON, 4326) NOT NULL,
    dataset_id              BIGINT NOT NULL REFERENCES regional_datasets (id)
);

CREATE INDEX census_sectors_geom_idx ON census_sectors USING GIST (geom);
CREATE INDEX census_sectors_municipality_idx ON census_sectors (municipality_ibge_code);

-- Indicadores por setor. Sem FK para census_sectors: o IBGE publica malha e
-- agregados separadamente e a ordem de importação não deve importar.
CREATE TABLE census_sector_indicators (
    id                  BIGSERIAL PRIMARY KEY,
    sector_code         TEXT NOT NULL,
    indicator           TEXT NOT NULL,
    -- NULL quando a fonte suprime o valor; value_text guarda o marcador.
    value               DOUBLE PRECISION,
    value_text          TEXT,
    source_variable     TEXT NOT NULL,
    source              TEXT NOT NULL,
    dataset_name        TEXT NOT NULL,
    dataset_id          BIGINT NOT NULL REFERENCES regional_datasets (id),
    UNIQUE (sector_code, indicator, source, dataset_name)
);

CREATE INDEX census_sector_indicators_sector_idx ON census_sector_indicators (sector_code);

-- Equipamentos urbanos (UBS, escolas, parques...).
CREATE TABLE urban_services (
    id                      BIGSERIAL PRIMARY KEY,
    source                  TEXT NOT NULL,
    dataset_name            TEXT NOT NULL,
    external_id             TEXT NOT NULL,
    name                    TEXT,
    category                TEXT NOT NULL,
    subcategory             TEXT,
    address                 TEXT,
    municipality_ibge_code  TEXT NOT NULL,
    geom                    GEOGRAPHY(POINT, 4326) NOT NULL,
    attributes              JSONB NOT NULL DEFAULT '{}'::jsonb,
    dataset_id              BIGINT NOT NULL REFERENCES regional_datasets (id),
    UNIQUE (source, dataset_name, external_id)
);

CREATE INDEX urban_services_geom_idx ON urban_services USING GIST (geom);
CREATE INDEX urban_services_category_idx ON urban_services (municipality_ibge_code, category);

-- Áreas de risco ambiental (polígonos oficiais). severity é o texto da
-- fonte; não há escala numérica.
CREATE TABLE environmental_risk_areas (
    id                      BIGSERIAL PRIMARY KEY,
    source                  TEXT NOT NULL,
    dataset_name            TEXT NOT NULL,
    external_id             TEXT NOT NULL,
    risk_types              TEXT[] NOT NULL CHECK (cardinality(risk_types) > 0),
    source_labels           TEXT[] NOT NULL DEFAULT '{}',
    severity                TEXT,
    location_name           TEXT,
    mapped_on               DATE,
    municipality_ibge_code  TEXT NOT NULL,
    geom                    GEOMETRY(GEOMETRY, 4326) NOT NULL,
    attributes              JSONB NOT NULL DEFAULT '{}'::jsonb,
    dataset_id              BIGINT NOT NULL REFERENCES regional_datasets (id),
    UNIQUE (source, dataset_name, external_id)
);

CREATE INDEX environmental_risk_areas_geom_idx ON environmental_risk_areas USING GIST (geom);
CREATE INDEX environmental_risk_areas_geog_idx
    ON environmental_risk_areas USING GIST ((geom::geography));
CREATE INDEX environmental_risk_areas_municipality_idx
    ON environmental_risk_areas (municipality_ibge_code);
