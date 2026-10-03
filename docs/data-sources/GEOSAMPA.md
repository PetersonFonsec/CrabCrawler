# GeoSampa — Mapa Digital da Cidade de São Paulo

Última verificação: 2026-10-03 (GetCapabilities do WFS e catálogo de metadados
lidos pela web; o GetCapabilities veio truncado, por isso só parte das camadas
foi confirmada).

| | |
|---|---|
| URL oficial | <https://geosampa.prefeitura.sp.gov.br> |
| API | WFS (GeoServer): `https://wfs.geosampa.prefeitura.sp.gov.br/geoserver/geoportal/wfs` |
| Formato | GeoJSON (`outputFormat=application/json`), WFS 1.0.0 |
| SRID | SIRGAS 2000 / UTM 23S (EPSG:31983), lido do membro `crs`; convertido para 4326 no PostGIS |
| Granularidade | Ponto (equipamento) |
| Cobertura | Somente o município de São Paulo (3550308) |
| Atualização | Por camada; o catálogo indica atualização anual para saúde e mensal para risco |

## Camadas integradas (nome WFS confirmado)

| Camada | Categoria | Subcategoria |
|---|---|---|
| `equipamento_saude_ambulatorios_especializados` | HEALTH | Ambulatório especializado |
| `equipamento_saude_saude_mental` | HEALTH | Saúde mental |
| `equipamento_educacao_ceu` | EDUCATION | CEU |
| `equipamento_educacao_outros` | EDUCATION | Educação (outros) |
| `equipamento_cultura_bibliotecas` | CULTURE | Biblioteca |
| `equipamento_cultura_outros` | CULTURE | Cultura (outros) |
| `equipamento_esporte_centro_esportivo` | SPORT | Centro esportivo |
| `equipamento_esporte_clubes` | SPORT | Clube |
| `equipamento_esporte_clubesdacomunidade` | SPORT | Clube da comunidade |

Campos usados: id do Feature (identificador original), primeira propriedade
`nm_*`/`nome*` preenchida como nome, primeira com "endereco"/"logradouro"
como endereço. Todas as propriedades originais ficam em `attributes`.

## Existem no catálogo, mas sem nome WFS confirmado

- Saúde: "UBS/Posto/Centro de Saúde" (revisão 2025-06-18, anual), "Hospital",
  "Urgência/Emergência".
- Riscos: "Risco Geológico" e "Risco Hidrológico" (revisão 2024-12-06, mensal,
  EPSG:31983). O dicionário do risco geológico lista `tx_grau_de_risco_geologico`
  com valores r1 a r4 e "AREA ENCERRADA", `tx_tipo_processo_geologico`,
  `dt_vistoria`, `qt_moradia`.
- `risco_ocorrencia_alagamento` aparece no GetCapabilities, mas são ocorrências
  (não áreas) e o tipo de geometria não foi conferido.
- Parques e transporte: não foram encontradas camadas de equipamentos com nome
  confirmado.

Para incluir uma camada: confirme o nome no GetCapabilities
(`curl '<WFS>?service=WFS&request=GetCapabilities' | grep -o '<Name>geoportal:[^<]*'`)
e acrescente uma linha em `LAYERS` (`crates/crawler/src/regional/geosampa.rs`).

## Ingestão

```bash
cargo run -- crawl geosampa                          # todas as camadas confirmadas
cargo run -- crawl geosampa --layer equipamento_educacao_ceu
cargo run -- crawl geosampa --layer equipamento_educacao_ceu --file ceu.geojson
```

O GeoJSON baixado é gravado em `data/raw/geosampa/<camada>.geojson` antes do
parse. Uma camada que falha não impede as outras; cada uma tem seu registro em
`regional_imports`.

## Limitações

- Não cobre o ABC: para imóveis em São Bernardo, serviços saem como
  `NO_DATA / no_provider_for_location`.
- Equipamentos de municípios vizinhos não aparecem, mesmo perto da divisa.
- Nem todo tipo de equipamento foi importado; a resposta lista os datasets
  considerados em cada categoria.
