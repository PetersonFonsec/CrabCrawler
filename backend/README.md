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
cargo run -- crawl ibge              # população do Censo 2022 via API do IBGE
cargo run -- serve
```

## Endpoints

- `GET /health`
- `GET /sources` — fontes de dados e links de referência
- `GET /listings?municipality_ibge_code=3548708&neighborhood=rudge-ramos&transaction=sale&min_bedrooms=2&max_price=600000`
- `GET /listings/{id}` — imóvel + comparação de preço/m² no bairro + indicadores da região
- `GET /regions/{ibge_code}/indicators`

## Testes

```bash
cargo test
cargo clippy --all-targets
```
