# Regional Intelligence

Responde "como seria morar neste imóvel e nesta região?" com dados públicos
oficiais, ligados ao imóvel pela **coordenada do anúncio**. Três blocos:
perfil da região (setor censitário), serviços próximos e riscos ambientais.

Fontes e formatos: [`data-sources/`](data-sources/) ([IBGE](data-sources/IBGE.md),
[Seade](data-sources/SEADE.md), [GeoSampa](data-sources/GEOSAMPA.md),
[SGB](data-sources/SGB.md)).

## Princípios

- Granularidade real: dado de setor é apresentado como setor; densidade
  calculada pelo projeto vem marcada como `derived`.
- Sem notas ou escalas próprias: grau de risco e grupo do IPVS são os da fonte.
- `NO_DATA` (com motivo) é diferente de "a fonte foi consultada e não há nada".
  Nenhuma resposta diz que não há risco ou serviço por falta de dataset.
- Sem coordenada não há análise: nada de centroide de bairro ou de CEP.

## Arquitetura

```
fonte oficial ─► arquivo bruto (data/raw/…) ─► provider (crab-crawler::regional)
   ─► normalizador (crab-processing::regional) ─► RegionalRepository (PostGIS)
   ─► API (crab-api::regional)
```

- **Domínio** (`crab-domain::regional`): categorias de serviço, tipos de risco
  (COBRADE), capacidades, cobertura, política de atualização, procedência de
  dataset. `REGIONAL_PROVIDERS` descreve cada provider.
- **Providers** (`crab-crawler::regional`): traits `CensusSectorProvider`,
  `DemographicDataProvider`, `InfrastructureDataProvider`,
  `EnvironmentalRiskProvider` (todas sobre `RegionalDataProvider`, que expõe
  cobertura e capacidades). Implementações: `IbgeSectorMeshProvider`,
  `IbgeBasicAggregatesProvider`, `SeadeIpvsProvider`, `GeoSampaProvider`,
  `SgbRiskProvider`. Um provider de prefeitura (São Bernardo, Santo André...)
  é mais uma implementação + uma linha em `REGIONAL_PROVIDERS`.
- **Normalização**: código de setor (15 dígitos), números no formato
  brasileiro, sigilo (`X`), tipologias COBRADE → tipo de risco, relatório de
  registros descartados.
- **Persistência**: transação por dataset; savepoint por registro (geometria
  recusada pelo PostGIS descarta só aquele registro).

## Banco (migration `20261004000000_regional.sql`)

| Tabela | Conteúdo | Índices |
|---|---|---|
| `regional_datasets` | fonte, dataset, versão, URL, data de referência, coleta, granularidade, categorias | `UNIQUE (source, dataset_name, dataset_version)` |
| `regional_dataset_coverage` | municípios cobertos por cada versão | PK + índice por município |
| `regional_imports` | cada execução: status, lidos/gravados/ignorados/removidos, erro, relatório | `(source, dataset_name, started_at DESC)` |
| `census_sectors` | setor, município, área, `GEOMETRY(MULTIPOLYGON, 4326)` | **GIST** em `geom`, município |
| `census_sector_indicators` | indicador por setor, valor, texto original, variável da fonte | `UNIQUE (sector_code, indicator, source, dataset_name)`, setor |
| `urban_services` | equipamento, categoria, endereço, `GEOGRAPHY(POINT)`, atributos originais | **GIST** em `geom`, `(município, categoria)` |
| `environmental_risk_areas` | tipos, rótulos, grau, data do mapeamento, `GEOMETRY(GEOMETRY, 4326)` | **GIST** em `geom` e em `geom::geography`, município |

Consultas: `ST_Covers` (ponto no setor), `ST_DWithin` + `ST_Distance` em
`geography` (serviços no raio, metros), `<->` (mais próximo por categoria),
`ST_Intersects` + `ST_Distance` (dentro da área de risco / área mais próxima).
Não há cache nem materialized view: com índices GIST, as consultas por imóvel
são pontuais.

### Idempotência e versões

- Versão = nome do arquivo publicado + SHA-256 do bruto. Mesmo arquivo, mesma
  versão: as linhas são atualizadas pelas chaves naturais, nada duplica.
- Versão nova: as linhas do dataset nos municípios cobertos passam para a nova
  versão e as que sumiram da fonte são removidas, na mesma transação.
- Cada provider tem a própria política de atualização (Censo e IPVS decenais,
  GeoSampa mensal, SGB anual/sob demanda). `/regional/sources` mostra a última
  importação e `refresh_due`. O projeto não tem agendador; as importações são
  comandos.

## Cobertura

| | São Paulo (capital) | São Bernardo do Campo |
|---|---|---|
| IBGE (setores, demografia) | sim | sim |
| Seade IPVS | sim | sim |
| GeoSampa (serviços) | sim (saúde, educação, cultura, esporte) | **não** |
| SGB (risco) | depende do mapeamento do SGB | sim (32 setores, 2014) |

Motivos de `NO_DATA`: `property_without_coordinates`,
`property_without_municipality`, `no_provider_for_location`, `not_imported`,
`outside_imported_sectors`, `suppressed_by_source`, `not_published`.

Riscos têm `assessment`: `INSIDE_MAPPED_RISK_AREA`, `NOT_IN_MAPPED_RISK_AREA`
(há dataset e o imóvel não está em área mapeada, o que **não** é ausência de
risco) ou `NO_DATA`.

## Importação

```bash
cd backend
cargo run -- migrate
cargo run -- crawl ibge-setores --file SP_setores_CD2022.gpkg --scope 3548708
cargo run -- crawl ibge-agregados --file Agregados_por_setores_basico_BR_20260520.zip --scope 3548708
cargo run -- crawl seade-ipvs --file ipvs_2022.csv --scope 3548708
cargo run -- crawl sgb-risco --municipality 3548708
cargo run -- crawl geosampa
```

Fixtures fictícias (mesmo formato das fontes) em `backend/fixtures/regional/`,
geradas por `make_fixtures.py`:

```bash
cargo run -- crawl ibge-setores --file fixtures/regional/ibge_setores_sample.gpkg --scope 35
cargo run -- crawl ibge-agregados --file fixtures/regional/ibge_agregados_basico_sample.zip --scope 35
cargo run -- crawl seade-ipvs --file fixtures/regional/seade_ipvs_sample.csv --scope 35
cargo run -- crawl sgb-risco --file fixtures/regional/sgb_risco_sample.geojson
cargo run -- crawl geosampa --layer equipamento_cultura_bibliotecas \
  --file fixtures/regional/geosampa_bibliotecas_sample.geojson
```

## API

| Rota | O que devolve |
|---|---|
| `GET /listings/{id}/region` | `profile` + `services` (raio 1 km) + `environmental_risks` (2 km) |
| `GET /listings/{id}/region/profile` | setor censitário, indicadores com fonte e variável original, o que falta e por quê |
| `GET /listings/{id}/region/services?radius=1000&category=HEALTH,EDUCATION` | por categoria: status, mais próximo, itens no raio, datasets considerados |
| `GET /listings/{id}/region/risks?max_distance=2000&type=FLOOD` | `assessment`, por tipo a área que contém o imóvel ou a mais próxima |
| `GET /listings/{id}/intelligence` | `{ property, region }` |
| `GET /regional/sources` | providers (cobertura, capacidades, atualização, limitações) e últimas importações |

O padrão de URL segue o projeto (`/listings/{id}/…`, snake_case no JSON).

Exemplo (fixtures), `GET /listings/{id}/region/risks`:

```json
{
  "environmental_risks": {
    "assessment": "NOT_IN_MAPPED_RISK_AREA",
    "message": "O imóvel não está dentro de nenhuma área de risco mapeada pelas fontes importadas. Isso não significa ausência de risco: as fontes mapeiam apenas áreas específicas.",
    "search_radius_m": 2000.0,
    "risks": [{
      "type": "FLOOD", "label": "Inundação",
      "inside_risk_area": false, "distance_meters": 326.0,
      "severity": "Alto", "source_labels": ["Inundação"], "mapped_on": "2014-05-14",
      "external_id": "SP_SBC_SR_FICT_02",
      "source": { "source": "sgb_setorizacao_risco", "dataset_name": "setorizacao_risco", "dataset_version": "sgb_risco_sample.geojson@sha256:…", "…": "…" }
    }]
  }
}
```

`GET /listings/{id}/region/services` num imóvel da capital:

```json
{ "category": "CULTURE", "status": "AVAILABLE",
  "nearest": { "name": "Biblioteca Fictícia Centro", "distance_m": 232.0, "distance_label": "230 m", "…": "…" },
  "within_radius": [ … ] }
{ "category": "TRANSPORT", "status": "NO_DATA", "reason": "no_provider_for_location" }
```

## Comparação futura

Cada bloco é calculado por imóvel e tem forma estável (`status`, valores com
fonte, `distance_m`, `assessment`), então comparar A × B × C é chamar
`/intelligence` para cada um e alinhar campos. Não há algoritmo de "melhor
imóvel".

## Limitações

- Este ambiente de desenvolvimento não acessa IBGE, Seade, GeoSampa nem SGB:
  os importadores foram testados com fixtures fictícias no formato
  documentado. A primeira importação real precisa rodar numa máquina com
  acesso.
- Colunas do GeoPackage do IBGE e do CSV do IPVS não foram conferidas no
  arquivo real (detecção por nome, com erro claro).
- Sem renda por setor (arquivo não publicado na pasta atual do IBGE).
- Serviços só na capital (GeoSampa) e só nas camadas com nome WFS confirmado;
  UBS e hospitais ainda não entram.
- Risco no ABC só pelo SGB (2014). Riscos do GeoSampa (geológico/hidrológico)
  ficam para quando o nome das camadas for confirmado.
- Distâncias em linha reta, não por rota.

## Próximos passos

1. Rodar as importações reais (IBGE, Seade, SGB, GeoSampa) e ajustar nomes de
   colunas e camadas que divergirem.
2. Serviços no ABC com fontes nacionais georreferenciadas: CNES (saúde) e
   Catálogo de Escolas do INEP; transporte via GTFS (EMTU/SPTrans) e estações
   CPTM/Metrô.
3. Camadas de risco geológico e hidrológico do GeoSampa e da Defesa Civil de
   São Bernardo, se publicadas.
4. Renda por setor quando o IBGE publicar o arquivo de rendimento.
5. Tela de "Como é morar aqui" no frontend, seguindo a identidade visual.
