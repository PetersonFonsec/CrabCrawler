-- Property Sources: separa o imóvel físico (properties) do anúncio
-- (listings), guarda proveniência, status, hash, histórico de preço,
-- parceiros e execuções de sincronização. Nenhum dado existente é apagado.

-- Imóvel físico. A coordenada leva origem e precisão para distinguir ponto
-- exato de aproximação.
CREATE TABLE properties (
    id                      UUID PRIMARY KEY,
    property_type           TEXT NOT NULL DEFAULT 'other',
    area_m2                 DOUBLE PRECISION CHECK (area_m2 IS NULL OR area_m2 > 0),
    lot_area_m2             DOUBLE PRECISION CHECK (lot_area_m2 IS NULL OR lot_area_m2 > 0),
    bedrooms                INTEGER CHECK (bedrooms IS NULL OR bedrooms >= 0),
    bathrooms               INTEGER CHECK (bathrooms IS NULL OR bathrooms >= 0),
    suites                  INTEGER CHECK (suites IS NULL OR suites >= 0),
    parking_spaces          INTEGER CHECK (parking_spaces IS NULL OR parking_spaces >= 0),
    street                  TEXT,
    street_number           TEXT,
    complement              TEXT,
    neighborhood            TEXT,
    neighborhood_slug       TEXT,
    municipality            TEXT,
    municipality_ibge_code  TEXT,
    state                   TEXT,
    postal_code             TEXT CHECK (postal_code IS NULL OR postal_code ~ '^[0-9]{8}$'),
    geom                    GEOGRAPHY(POINT, 4326),
    coordinate_source       TEXT CHECK (coordinate_source IN
                                ('PROPERTY_SOURCE', 'GEOCODING', 'POSTAL_CODE_CENTROID', 'MANUAL')),
    coordinate_precision    TEXT CHECK (coordinate_precision IN
                                ('EXACT', 'STREET', 'POSTAL_CODE', 'APPROXIMATE', 'REPORTED')),
    created_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK ((geom IS NULL) = (coordinate_source IS NULL))
);

CREATE INDEX properties_municipality_idx ON properties (municipality_ibge_code, neighborhood_slug);
CREATE INDEX properties_postal_code_idx ON properties (postal_code);
CREATE INDEX properties_geom_idx ON properties USING GIST (geom);

-- Parceiros que fornecem imóveis. Sem credenciais: `configuration` guarda
-- só dados não sensíveis e, quando preciso, o NOME da variável de ambiente
-- com o segredo.
CREATE TABLE property_source_partners (
    id              UUID PRIMARY KEY,
    slug            TEXT NOT NULL UNIQUE CHECK (slug ~ '^[a-z0-9][a-z0-9-]{0,62}$'),
    name            TEXT NOT NULL,
    partner_type    TEXT NOT NULL CHECK (partner_type IN
                        ('REAL_ESTATE_AGENCY', 'BROKER', 'CRM', 'MARKETPLACE')),
    status          TEXT NOT NULL DEFAULT 'ACTIVE' CHECK (status IN ('ACTIVE', 'PAUSED', 'DISABLED')),
    provider        TEXT NOT NULL CHECK (provider IN ('MANUAL', 'VRSYNC', 'API', 'FIXTURE')),
    configuration   JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Backfill: cada anúncio existente vira um imóvel com o mesmo id.
INSERT INTO properties (id, property_type, area_m2, bedrooms, bathrooms, parking_spaces,
    street, neighborhood, neighborhood_slug, municipality, municipality_ibge_code, state,
    postal_code, geom, coordinate_source, coordinate_precision, created_at, updated_at)
SELECT id, kind, NULLIF(area_m2, 0), bedrooms, bathrooms, parking_spots,
    NULL, neighborhood, neighborhood_slug, municipality, municipality_ibge_code, state,
    CASE WHEN postal_code ~ '^[0-9]{8}$' THEN postal_code END,
    geom,
    CASE WHEN geom IS NOT NULL THEN 'PROPERTY_SOURCE' END,
    CASE WHEN geom IS NOT NULL THEN 'REPORTED' END,
    collected_at, updated_at
FROM listings;

-- listings passa a ser o anúncio.
ALTER TABLE listings
    ADD COLUMN property_id          UUID REFERENCES properties (id) ON DELETE RESTRICT,
    ADD COLUMN partner_id           UUID REFERENCES property_source_partners (id) ON DELETE RESTRICT,
    ADD COLUMN status               TEXT NOT NULL DEFAULT 'ACTIVE'
                                    CHECK (status IN ('ACTIVE', 'INACTIVE', 'REMOVED', 'UNKNOWN')),
    ADD COLUMN condominium_fee_brl  DOUBLE PRECISION,
    ADD COLUMN property_tax_brl     DOUBLE PRECISION,
    ADD COLUMN description          TEXT,
    ADD COLUMN notes                TEXT,
    ADD COLUMN images               JSONB NOT NULL DEFAULT '[]'::jsonb,
    ADD COLUMN original             JSONB NOT NULL DEFAULT '{}'::jsonb,
    ADD COLUMN raw_payload          JSONB NOT NULL DEFAULT 'null'::jsonb,
    ADD COLUMN content_hash         TEXT,
    ADD COLUMN first_seen_at        TIMESTAMPTZ,
    ADD COLUMN last_seen_at         TIMESTAMPTZ,
    ADD COLUMN imported_at          TIMESTAMPTZ;

UPDATE listings SET
    property_id = id,
    source = CASE WHEN source = 'listing_fixture' THEN 'FIXTURE' ELSE upper(source) END,
    original = jsonb_build_object('kind', kind),
    first_seen_at = collected_at,
    last_seen_at = collected_at,
    imported_at = collected_at;

-- Um preço desconhecido não cabe num anúncio; o fixture sempre tem preço.
DELETE FROM listings WHERE price_brl IS NULL;

ALTER TABLE listings
    ALTER COLUMN property_id SET NOT NULL,
    ALTER COLUMN price_brl SET NOT NULL,
    ALTER COLUMN first_seen_at SET NOT NULL,
    ALTER COLUMN last_seen_at SET NOT NULL,
    ALTER COLUMN imported_at SET NOT NULL,
    ADD CONSTRAINT listings_price_positive CHECK (price_brl > 0),
    ADD CONSTRAINT listings_source_check CHECK (source IN ('MANUAL', 'VRSYNC', 'API', 'FIXTURE')),
    ADD CONSTRAINT listings_transaction_check CHECK (transaction IN ('sale', 'rent'));

-- Dados físicos agora vivem em properties.
DROP INDEX IF EXISTS listings_municipality_idx;
DROP INDEX IF EXISTS listings_geom_idx;
ALTER TABLE listings
    DROP COLUMN kind,
    DROP COLUMN area_m2,
    DROP COLUMN bedrooms,
    DROP COLUMN bathrooms,
    DROP COLUMN parking_spots,
    DROP COLUMN state,
    DROP COLUMN municipality,
    DROP COLUMN municipality_ibge_code,
    DROP COLUMN neighborhood,
    DROP COLUMN neighborhood_slug,
    DROP COLUMN postal_code,
    DROP COLUMN geom;

-- Identidade do anúncio: fonte + parceiro + id externo + finalidade
-- ("Sale/Rent" no VRSync gera dois anúncios com o mesmo ListingID).
ALTER TABLE listings DROP CONSTRAINT listings_source_external_id_key;
ALTER TABLE listings ADD CONSTRAINT listings_identity_key
    UNIQUE NULLS NOT DISTINCT (source, partner_id, external_id, transaction);

CREATE INDEX listings_property_idx ON listings (property_id);
CREATE INDEX listings_partner_status_idx ON listings (source, partner_id, status, last_seen_at);

-- Histórico de preço: uma linha por mudança (nunca por observação repetida).
CREATE TABLE listing_price_history (
    id          BIGSERIAL PRIMARY KEY,
    listing_id  UUID NOT NULL REFERENCES listings (id) ON DELETE CASCADE,
    price_brl   DOUBLE PRECISION NOT NULL CHECK (price_brl > 0),
    observed_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX listing_price_history_idx ON listing_price_history (listing_id, observed_at DESC);

INSERT INTO listing_price_history (listing_id, price_brl, observed_at)
SELECT id, price_brl, collected_at FROM listings;

-- Execuções de sincronização: o que veio, de onde, e o que aconteceu.
CREATE TABLE property_sync_runs (
    id              BIGSERIAL PRIMARY KEY,
    provider        TEXT NOT NULL,
    partner_id      UUID REFERENCES property_source_partners (id) ON DELETE SET NULL,
    status          TEXT NOT NULL CHECK (status IN ('RUNNING', 'SUCCEEDED', 'FAILED')),
    received        INTEGER NOT NULL DEFAULT 0,
    created         INTEGER NOT NULL DEFAULT 0,
    updated         INTEGER NOT NULL DEFAULT 0,
    unchanged       INTEGER NOT NULL DEFAULT 0,
    invalid         INTEGER NOT NULL DEFAULT 0,
    deactivated     INTEGER NOT NULL DEFAULT 0,
    failed          INTEGER NOT NULL DEFAULT 0,
    duration_ms     BIGINT,
    error           TEXT,
    -- Amostra dos erros por item (limitada no código).
    item_errors     JSONB NOT NULL DEFAULT '[]'::jsonb,
    -- Ex.: versão/data do feed, motivo de a inativação ter sido suspensa.
    details         JSONB NOT NULL DEFAULT '{}'::jsonb,
    started_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at     TIMESTAMPTZ
);

CREATE INDEX property_sync_runs_idx ON property_sync_runs (provider, partner_id, started_at DESC);

-- Cache de geocoding: as políticas dos serviços públicos pedem cache e
-- poucas requisições.
CREATE TABLE geocoding_cache (
    query_key   TEXT PRIMARY KEY,
    provider    TEXT NOT NULL,
    found       BOOLEAN NOT NULL,
    lat         DOUBLE PRECISION,
    lon         DOUBLE PRECISION,
    precision   TEXT,
    payload     JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
