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
| SSP-SP — Dados Mensais | dataset oficial (exportação manual) | ocorrências por natureza, município e delegacia, por mês | importador de CSV pronto; sem API pública ([detalhes](SEGURANCA.md)) |
| Sinesp VDE (MJSP) | planilha anual oficial | eventos criminais por município e mês | importador XLSX pronto, com download direto do gov.br |
| IBGE — malha e agregados por setor (Censo 2022) | GeoPackage + CSV oficiais | perfil da região por setor censitário | importadores prontos ([detalhes](data-sources/IBGE.md)) |
| Seade — IPVS 2022 | CSV oficial | vulnerabilidade social por setor | importador pronto, colunas a conferir ([detalhes](data-sources/SEADE.md)) |
| GeoSampa (capital) | WFS oficial | equipamentos urbanos | importador pronto para as camadas confirmadas ([detalhes](data-sources/GEOSAMPA.md)) |
| SGB — Setorização de Risco | API ArcGIS REST | áreas de risco (inclui SBC) | importador pronto ([detalhes](data-sources/SGB.md)) |

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
2. Conferir os importadores de segurança contra arquivos reais e importar os
   microdados georreferenciados da SSP ([próximos passos](SEGURANCA.md#próximos-passos)).
3. ~~Importar malhas do IBGE e relacionar por coordenada (PostGIS).~~ Feito no
   [Regional Intelligence](REGIONAL.md); falta rodar com os arquivos reais.
4. Estimativas anuais de população para a taxa por 100 mil de cada ano.
5. Telas de busca, mapa e detalhe no frontend.
