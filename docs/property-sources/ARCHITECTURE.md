# Property Sources: arquitetura

Como imóveis entram no CrabCrawler. Só fontes autorizadas: API oficial de
parceiro, feed VRSync/XML de parceiro, cadastro manual e, depois, CRMs.
**Nada de scraping** de portal que proíbe automação; receber uma URL não
autoriza acessá-la.

## Pipeline

```text
 Fonte autorizada           Provider (crab-crawler)        Ingestão (crab-ingest)
 ─────────────────          ───────────────────────        ──────────────────────
 Formulário /analisar ──►  ManualPropertyInput ──┐
 Feed VRSync do parceiro ► VrsyncProvider ───────┼──► RawProperty (texto cru, como veio)
 API do parceiro (futuro)► ApiClient + ApiMapping┘            │
                                                              ▼
                                         PropertyNormalizer (crab-processing)
                                         valores tipados + originais + avisos
                                                              │
                                     hash igual ao anterior? ─┤── sim: só last_seen_at
                                                              ▼
                                   CEP → endereço (ViaCEP, opcional)
                                   endereço → lat/lon (Nominatim, opcional, com cache)
                                                              │
                                                              ▼
                     properties ◄── 1:N ── listings ── 1:N ── listing_price_history
                          │
                          ▼
            Regional Intelligence (/properties/{id}/region) e comparador
```

## Imóvel × anúncio

- **`Property`** (`properties`): o imóvel físico. Tipo, área, quartos,
  endereço, coordenada (com origem e precisão). Não sabe de onde veio.
- **`PropertyListing`** (`listings`): uma oferta desse imóvel numa fonte.
  Finalidade, preço, taxas, status, URL de referência, procedência, hash.
- Um item VRSync `Sale/Rent` vira **um imóvel com dois anúncios**.
- Cadastro manual cria um imóvel com um anúncio `MANUAL`. Não há
  deduplicação entre fontes ainda: o mesmo apartamento vindo de dois
  parceiros vira dois imóveis (ver limitações).

## Providers e capacidades

| Fonte (`PropertySource`) | Capacidades | Status |
|---|---|---|
| `MANUAL` | `MANUAL_ENTRY` | pronto ([MANUAL.md](MANUAL.md)) |
| `VRSYNC` | `BULK_IMPORT` (feed completo) | pronto, testado com fixtures fictícias ([VRSYNC.md](VRSYNC.md)) |
| `API` | `SEARCH`, `FETCH_BY_ID`, `INCREMENTAL_SYNC` conforme a API | só infraestrutura; nenhuma API real ([API.md](API.md)) |
| `FIXTURE` | `BULK_IMPORT` parcial | desenvolvimento (`fixtures/listings.json`) |

Trait em `backend/crates/crawler/src/property_sources/mod.rs`:
`PropertySourceProvider { source, capabilities, supports, fetch, fetch_by_id }`.
`fetch` devolve um `FetchBatch { items, complete, metadata }`: `complete`
diz se o lote é o estoque inteiro do parceiro (só então itens ausentes podem
ser inativados). Itens ilegíveis viram `ItemReadError` sem derrubar o lote.

## Procedência

Cada anúncio guarda `source`, `external_id`, `source_url`, `partner_id`,
`first_seen_at`, `last_seen_at`, `imported_at`, `updated_at`,
`content_hash`, `original` (valores como vieram, por campo) e `raw_payload`
(item bruto sem dados de contato). Identidade:
`UNIQUE NULLS NOT DISTINCT (source, partner_id, external_id, transaction)`.

## Status e histórico

- `ACTIVE`, `INACTIVE`, `REMOVED`, `UNKNOWN`. Um anúncio que sumiu do feed vira
  `INACTIVE`; **nunca é apagado**. Volta a `ACTIVE` se reaparecer.
- Item presente mas ilegível vira `UNKNOWN` (não sabemos se saiu).
- Inativação só com lote completo, ao menos um item válido e menos de 50%
  do estoque ativo sumindo (com 10+ ativos); acima disso exige
  `--allow-mass-deactivation`. Protege contra feed truncado ou vazio.
- `listing_price_history`: uma linha na criação e uma a cada mudança de
  preço. Preço igual não gera linha.

## Jobs de sincronização

`crabcrawler sync properties --partner <slug>`: um job por parceiro, um
instante por execução, registrado em `property_sync_runs` com
`received, created, updated, unchanged, invalid, deactivated, failed,
duration_ms`, status (`SUCCEEDED`, `PARTIAL`, `FAILED`) e até 200 erros por
item. Logs estruturados (`tracing`), sem tokens: a URL é redigida e o
cabeçalho de autorização vem de uma variável de ambiente cujo **nome** é o
que fica no banco.

## Geocoding

Separado dos providers ([GEOCODING.md](GEOCODING.md)). Coordenada guarda
`coordinate_source` (`PROPERTY_SOURCE`, `GEOCODING`, `POSTAL_CODE_CENTROID`,
`MANUAL`) e `coordinate_precision` (`EXACT`, `STREET`, `POSTAL_CODE`,
`APPROXIMATE`, `REPORTED`). A análise regional recusa precisão `POSTAL_CODE`
e `APPROXIMATE` com o motivo `PROPERTY_COORDINATES_APPROXIMATE`.

## Tabelas (migration `20261005000000_property_sources.sql`)

| Tabela | Conteúdo | Índices principais |
|---|---|---|
| `properties` | imóvel físico, `geom` PostGIS | GiST em `geom`; (`municipality_ibge_code`, `neighborhood_slug`) |
| `listings` | anúncios (tabela existente, migrada) | identidade única acima; `property_id`; (`partner_id`, `status`) |
| `listing_price_history` | preço por observação | (`listing_id`, `observed_at`) |
| `property_source_partners` | parceiros, provider, configuração **sem segredo** | `slug` único |
| `property_sync_runs` | métricas por execução | (`partner_id`, `started_at`) |
| `geocoding_cache` | resultado por endereço normalizado, inclusive "não achou" | chave primária |

A migration cria um imóvel para cada anúncio existente (mesmo id),
renomeia a fonte `listing_fixture` para `FIXTURE` e copia o preço atual para
o histórico.

## Endpoints

Públicos (só leitura e cadastro manual):

- `POST /properties/manual` (corpo até 32 KB): 201, ou 422 com `errors`/`warnings` por campo
- `GET /properties?municipality_ibge_code=&neighborhood=&property_type=&transaction=&source=&status=&min_bedrooms=&max_price=&limit=`
- `GET /properties/{id}`: imóvel, anúncios, histórico, "preço reduzido em X%", comparação de preço/m²
- `GET /properties/{id}/listings`
- `GET /properties/{id}/region` e `/properties/{id}/intelligence`: Regional Intelligence

Administração (parceiros, sincronização, execuções) **só pela CLI**; não há
rota HTTP para isso.

## Comparador

A comparação de preço/m² usa anúncios `ACTIVE` do mesmo bairro e finalidade,
de **qualquer fonte**, excluindo o próprio imóvel. Origem não pesa.

## Limitações conhecidas

- Sem autenticação: o cadastro manual é anônimo e público.
- Sem deduplicação de imóveis entre fontes.
- Centroide de CEP (`POSTAL_CODE_CENTROID`) previsto no modelo, sem fonte implementada.
- ViaCEP e Nominatim desligados por padrão (`ADDRESS_LOOKUP=none`, `GEOCODER=none`).
- `Retry-After` só no formato em segundos; data HTTP cai no backoff padrão.
- Nenhuma API real integrada; nenhum feed real testado.
