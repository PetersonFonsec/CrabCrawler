# CrabCrawler backend 🦀

Monólito modular em Rust: coleta anúncios e dados públicos, normaliza a
localização, cruza os datasets e expõe tudo numa API HTTP.

O plano do MVP está em [`../docs/MVP.md`](../docs/MVP.md).

## Crates

| Crate | Responsabilidade |
|---|---|
| `crab-domain` | Modelo de domínio puro (anúncio, localização, região, indicador, procedência). Sem I/O. |
| `crab-crawler` | Fontes de dados. Cliente HTTP com rate limiting, retry com backoff e logs. |
| `crab-processing` | Normalização de localização (texto → município/bairro/código IBGE) e enriquecimento. |
| `crab-persistence` | PostgreSQL/PostGIS via sqlx, migrations e repositórios. |
| `crab-api` | API HTTP em Axum. |
| `crabcrawler` (`crates/app`) | Binário único com CLI: `migrate`, `crawl`, `serve`. |

Dependências apontam sempre para o domínio; só o binário conhece todos os
crates. Separar crawler ou processamento em serviços no futuro é questão de
criar outro binário.

## Rodando localmente

```bash
cd backend
cp .env.example .env
docker compose up -d db
export $(cat .env | xargs)

cargo run -- migrate
cargo run -- crawl listings          # fixtures/listings.json
cargo run -- crawl ssp-sp            # fixtures/ssp_sp_sample.csv (amostra fictícia)
cargo run -- crawl sinesp --file fixtures/sinesp_vde_sample.xlsx   # amostra fictícia
cargo run -- crawl sinesp --year 2025 --ibge-gazetteer             # base real do gov.br
cargo run -- crawl ibge              # população do Censo 2022 via API do IBGE

# Regional Intelligence (detalhes em ../docs/REGIONAL.md); fixtures fictícias:
cargo run -- crawl ibge-setores --file fixtures/regional/ibge_setores_sample.gpkg --scope 35
cargo run -- crawl ibge-agregados --file fixtures/regional/ibge_agregados_basico_sample.zip --scope 35
cargo run -- crawl seade-ipvs --file fixtures/regional/seade_ipvs_sample.csv --scope 35
cargo run -- crawl sgb-risco --file fixtures/regional/sgb_risco_sample.geojson
cargo run -- crawl geosampa --layer equipamento_cultura_bibliotecas --file fixtures/regional/geosampa_bibliotecas_sample.geojson
# dados reais: crawl sgb-risco --municipality 3548708 | crawl geosampa | arquivos do IBGE/Seade
cargo run -- serve
```

## Endpoints

- `GET /health`
- `GET /sources` — fontes de dados e links de referência
- `GET /listings?municipality_ibge_code=3548708&neighborhood=rudge-ramos&transaction=sale&min_bedrooms=2&max_price=600000`
- `GET /listings/{id}` — imóvel + comparação de preço/m² no bairro + indicadores da região
- `GET /regions/{ibge_code}/indicators`

Segurança pública (detalhes em [`../docs/SEGURANCA.md`](../docs/SEGURANCA.md)):

- `GET /listings/{id}/security`
- `GET /regions/{ibge_code}/security?year=2025` (`&level=police_district&code=...` para delegacia)
- `GET /regions/{ibge_code}/security/history?crime_type=robbery`
- `GET /security/compare?municipalities=3548708,3547809` ou `?listings=<id>,<id>`
- `GET /security/sources`

Regional Intelligence (detalhes em [`../docs/REGIONAL.md`](../docs/REGIONAL.md)):

- `GET /listings/{id}/region` — perfil + serviços + riscos
- `GET /listings/{id}/region/profile`
- `GET /listings/{id}/region/services?radius=1000&category=HEALTH`
- `GET /listings/{id}/region/risks?max_distance=2000&type=FLOOD`
- `GET /listings/{id}/intelligence` — imóvel + região
- `GET /regional/sources`

## Testes

```bash
cargo test
cargo clippy --all-targets
cargo fmt --all --check

# teste de ponta a ponta (fixtures → banco → API) num banco descartável com PostGIS
TEST_DATABASE_URL=postgres://crab:crab@localhost:5432/crabcrawler_test cargo test -p crab-api
```
