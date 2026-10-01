# Plano do MVP

## Escopo

- **Região:** São Bernardo do Campo - SP (código IBGE `3548708`).
- **Pergunta que o MVP responde:** dado um anúncio, como o preço dele se
  compara com imóveis semelhantes no bairro, e quais indicadores públicos
  existem para a região onde ele está, com a origem de cada número.

## Fontes

| Fonte | Tipo | Uso no MVP | Status |
|---|---|---|---|
| Anúncios (fixture JSON) | arquivo local | desenvolver normalização, enriquecimento e API | pronto |
| Anúncios (portal real) | a definir | primeira fonte real de anúncios | **decisão pendente** |
| IBGE — API de Localidades e Agregados (SIDRA) | API oficial | código IBGE dos municípios, população do Censo 2022 | pronto |
| SSP-SP — estatísticas por delegacia | dataset oficial (planilhas) | roubos, furtos, roubo de veículo, homicídios por mês | importador de CSV pronto; download automático pendente |

**Sobre a fonte real de anúncios:** os grandes portais (ZAP, VivaReal, OLX,
QuintoAndar) proíbem coleta automatizada nos termos de uso. Opções, em ordem
de preferência: (1) uma API ou feed autorizado; (2) site de imobiliária local
cujo `robots.txt` e termos permitam; (3) anúncios inseridos manualmente. A
fonte entra como mais uma implementação de `ListingSource`, sem mudar o resto.

**Próximos datasets candidatos:** malhas de setores censitários e bairros
(IBGE), escolas (Censo Escolar/INEP), estabelecimentos de saúde (CNES),
pontos de ônibus/trem (GTFS da EMTU/CPTM), e CEP → coordenada
(BrasilAPI/ViaCEP + Nominatim com 1 req/s).

## Modelo de domínio

- `RawListing`: anúncio como veio da fonte, com `location_text` livre
  (`"São Bernardo do Campo - SP / Rudge Ramos"`).
- `Listing`: anúncio normalizado, ID determinístico por `(fonte, id externo)`.
- `Location`: UF, município, **código IBGE do município** (chave de junção
  principal), bairro + slug normalizado, CEP, coordenada opcional.
- `Region`: região com nível (`municipality`, `neighborhood`,
  `census_tract`, `police_district`) e código na fonte.
- `Indicator` / `RegionIndicator`: valor medido num período, ligado a uma
  região.
- `Provenance`: fonte, URL e data da coleta. Todo dado persistido tem uma.

## Fluxo

```
coletar anúncios ─► normalizar localização ─┐
                                            ├─► persistir (Postgres/PostGIS) ─► API (Axum) ─► interface
coletar dados públicos ─► mapear p/ região ─┘
```

1. **Coleta** (`crab-crawler`): cada fonte implementa `ListingSource` ou
   `IndicatorSource`. HTTP passa por um cliente com rate limiting, retry com
   backoff exponencial (429/5xx/rede) e logs.
2. **Normalização** (`crab-processing`): parse do texto de localização,
   slug sem acento, resolução município → código IBGE.
3. **Relacionamento:** hoje por código IBGE do município e slug do bairro.
   Próximo passo: geocodificar anúncios e usar `ST_Contains` contra as
   malhas de setores censitários e áreas de delegacia.
4. **Persistência** (`crab-persistence`): upsert idempotente em `listings`
   (`UNIQUE (source, external_id)`), `regions` e `region_indicators`.
5. **API** (`crab-api`): busca com filtros, detalhe do imóvel com comparação
   de preço/m² no bairro e indicadores da região, lista de fontes.
6. **Interface:** o Angular já existente na raiz do repositório consome a API.

## Próximos passos

1. Escolher e implementar a fonte real de anúncios.
2. Automatizar o download das planilhas da SSP-SP.
3. Importar malhas do IBGE e relacionar por coordenada (PostGIS).
4. Normalizar criminalidade por população (taxa por 100 mil habitantes).
5. Telas de busca, mapa e detalhe no frontend.
