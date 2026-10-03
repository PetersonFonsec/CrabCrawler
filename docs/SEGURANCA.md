# Módulo de Segurança Pública

Enriquecer imóveis com estatísticas criminais oficiais da região, sempre com
fonte, período, granularidade e limitações visíveis. O módulo **não calcula
score** e não classifica regiões como "seguras" ou "perigosas".

Primeiro recorte: estado de São Paulo, com São Bernardo do Campo (IBGE
`3548708`) como caso de teste.

## Fontes

Levantamento feito em 2026-10. O ambiente de desenvolvimento usado bloqueia
acesso direto a `ssp.sp.gov.br`, `gov.br/mj` e às APIs do IBGE, então nada
abaixo foi baixado ao vivo; o que não pôde ser confirmado está marcado.

| Fonte | Conteúdo | Granularidade | Período | Formato / acesso | Status no código |
|---|---|---|---|---|---|
| **SSP-SP — Dados Mensais** (Res. SSP 160/2001) — <https://www.ssp.sp.gov.br/estatistica/dados-mensais> | Ocorrências registradas por natureza (e vítimas para homicídio e latrocínio) | Estado, região, **município**, **delegacia** | Mensal, desde 2001 | Aplicação web com exportação manual. **Sem API pública documentada.** | `SspSpCsvProvider`: lê a exportação convertida para CSV |
| **SSP-SP — Consultas / microdados** — <https://www.ssp.sp.gov.br/estatistica/consultas> | Boletins individuais (`SPDadosCriminais_<ano>.xlsx`, `VeiculosSubtraidos_<ano>.xlsx`, `CelularesSubtraidos_<ano>.xlsx`) com natureza, data, cidade, bairro, logradouro, latitude/longitude | Ocorrência (ponto, quando preenchido) | Diário, por ano | XLSX, download manual. URL direta **não verificada**; colunas mudam entre anos | Tabela `crime_occurrences` preparada, **sem importador** |
| **Sinesp VDE** (MJSP) — <https://www.gov.br/mj/pt-br/assuntos/sua-seguranca/seguranca-publica/estatistica> | Eventos validados pelos gestores estaduais | **Município** (subconjunto de ~11 eventos) e UF (todos) | Mensal, desde 2015 | XLSX anual, download direto **verificado**: `.../dnsp-base-de-dados/bancovde-<ano>.xlsx/@@download/file` | `SinespVdeProvider`: arquivo local ou download por ano |
| **IBGE — Censo 2022** (SIDRA agregado 4709, var. 93) | População residente | Município | 2022 | API JSON | Já existia (`IbgeClient`); usado como denominador |

Colunas do Sinesp VDE: `uf, municipio, evento, data_referencia, agente, arma,
faixa_etaria, feminino, masculino, nao_informado, total_vitima, total,
total_peso, abrangencia, formulario`.

### Mapeamento de rótulos

O rótulo original da fonte é sempre gravado (`source_label`). O normalizador
(`crab-processing/src/security.rs`) traduz para tipos estáveis:

| `crime_type` | SSP-SP (natureza) | Sinesp VDE (evento) |
|---|---|---|
| `homicide` | HOMICÍDIO DOLOSO / Nº DE VÍTIMAS EM HOMICÍDIO DOLOSO | Homicídio doloso |
| `attempted_homicide` | TENTATIVA DE HOMICÍDIO | Tentativa de homicídio |
| `femicide` | FEMINICÍDIO | Feminicídio |
| `robbery_followed_by_death` | LATROCÍNIO / Nº DE VÍTIMAS EM LATROCÍNIO | Roubo seguido de morte (latrocínio) |
| `rape` | ESTUPRO | Estupro |
| `rape_of_vulnerable` | ESTUPRO DE VULNERÁVEL | — |
| `robbery` | ROUBO - OUTROS | — |
| `theft` | FURTO - OUTROS | — |
| `vehicle_robbery` | ROUBO DE VEÍCULO | Roubo de veículo |
| `vehicle_theft` | FURTO DE VEÍCULO | Furto de veículo |
| `cargo_robbery` | ROUBO DE CARGA | Roubo de carga |
| `financial_institution_robbery` | ROUBO A BANCO | Roubo a instituição financeira |
| `bodily_injury` | LESÃO CORPORAL DOLOSA | — |

Linhas "TOTAL DE …" da SSP são ignoradas porque somam outras linhas. Marcadores
de nota (`(2)`, `(3)`) e acentos não afetam o casamento. Rótulos sem mapeamento
são contados no relatório da importação, nunca descartados em silêncio.

## Fluxo

```
SecurityDataProvider (crab-crawler)     lê CSV/XLSX, devolve RawCrimeRecord com rótulo original
        │
        ▼
normalize_crime_records (crab-processing)   rótulo → crime_type + unidade; nome → código IBGE;
        │                                    nível geográfico real; soma duplicatas; relatório
        ▼
SecurityRepository (crab-persistence)   upsert idempotente em crime_statistics + security_dataset_imports
        │
        ▼
API (crab-api)                          soma mensal → anual, junta população, taxa, tendência, transparência
```

O crawler imobiliário não conhece segurança. O imóvel chega à região pelo
código IBGE do município (`listings.municipality_ibge_code`).

## Modelo

- `crime_statistics`: região (`regions`), `crime_type`, `counting_unit`
  (`occurrences` | `victims`), `count`, período (`period_start`, `period_end`,
  `period_granularity`), `source`, `source_label`, `source_url`,
  `dataset_version`, `collected_at`.
- `regions.level` guarda a granularidade real (`municipality`,
  `police_district`). Dado municipal nunca vira dado de bairro.
- `crime_occurrences`: ocorrência com `GEOGRAPHY(POINT)` e índice GIST, pronta
  para `ST_DWithin` em raios de 500 m, 1, 2 e 5 km.
- `security_dataset_imports`: log de cada importação (fonte, URL, versão,
  linhas lidas/gravadas/ignoradas e relatório JSON).

## Metodologia

- Contagens mensais são somadas por ano civil. Anos com menos de 12 meses
  saem com `complete: false` e não entram na tendência.
- `rate_per_100k = count / população × 100.000`, só para o nível municipal.
  A resposta traz separadamente `count`, `population` (valor, ano, fonte) e
  `rate_per_100k`.
- Hoje só existe a população do Censo 2022; a resposta avisa quando o ano do
  crime difere do ano da população.
- Fontes diferentes nunca são somadas; ocorrências e vítimas também não.
- Tendência (`/history`): variação entre o primeiro e o último dos três anos
  completos mais recentes, em taxa quando disponível. `consistent: true` quando
  todos os passos foram na mesma direção. É descrição dos registros, não
  avaliação de segurança.

## API

| Rota | O que devolve |
|---|---|
| `GET /listings/{id}/security?year=` | Localização do imóvel + relatório do município + avisos (ex.: "não há dado por bairro") |
| `GET /regions/{ibge_code}/security?year=&level=&code=` | Indicadores do ano (padrão: último ano completo), anos disponíveis, sub-regiões (delegacias) e bloco `transparency` |
| `GET /regions/{ibge_code}/security/history?crime_type=&level=&code=` | Série anual por fonte/tipo/unidade com `trend` |
| `GET /security/compare?municipalities=a,b` ou `?listings=a,b` | 2 a 5 relatórios lado a lado |
| `GET /security/sources` | Metodologia e limitações de cada fonte + última importação |

`level=police_district&code=<slug>` consulta uma delegacia (os códigos vêm em
`subregions`); essas respostas não têm taxa.

Todo relatório traz `transparency`: fontes usadas (com metodologia e
limitações), `last_updated`, `methodology` e `notes`.

## Como importar

```bash
cd backend
cargo run -- crawl ibge                                    # população (Censo 2022)
cargo run -- crawl ssp-sp --file dados_mensais.csv         # CSV no formato abaixo
cargo run -- crawl sinesp --year 2025 --ibge-gazetteer     # baixa do gov.br
cargo run -- crawl sinesp --file bancovde-2025.xlsx --municipality 3548708
```

CSV da SSP (`;`): `municipio_ibge;delegacia;ano;mes;natureza;quantidade`.
`delegacia` vazia = total do município. `natureza` exatamente como na página.

`fixtures/ssp_sp_sample.csv` e `fixtures/sinesp_vde_sample.xlsx` são
**fictícios**: usam os rótulos e cabeçalhos reais, com números inventados.

## Limitações conhecidas

- Registro policial não é criminalidade real; a subnotificação varia por crime.
- A SSP revisa números retroativamente e publica com atraso de 1 a 3 meses.
- Não há malha pública nem população das áreas de delegacia: sem taxa nesse nível.
- Não há estatística oficial por bairro. O bairro dos microdados é texto livre.
- A exportação dos Dados Mensais da SSP é manual (não há API documentada).
- Sinesp: só parte dos eventos sai por município; o município é casado pelo
  nome. Sem `--ibge-gazetteer` só os municípios do MVP são reconhecidos.
- Sinesp: para municípios de SP só há homicídio doloso, tentativa de
  homicídio, feminicídio e latrocínio (todos por vítima), conferido nas bases
  reais de 2023 a 2025. Roubo, furto e veículos dependem da SSP-SP.
- Eventos municipais do Sinesp ainda não mapeados: tentativa de feminicídio,
  lesão corporal seguida de morte e morte por intervenção de agente do Estado.
- Os rótulos da SSP no mapeamento seguem a tabela publicada, mas a exportação
  real não pôde ser baixada daqui para conferência.

## Próximos passos

1. Conferir os providers contra arquivos reais da SSP e do Sinesp e ajustar
   o mapeamento.
2. Estimativas anuais de população do IBGE para o denominador de cada ano.
3. Importar os microdados da SSP para `crime_occurrences`, medir a qualidade das
   coordenadas e só então expor contagens por raio do imóvel.
4. Geocodificar anúncios (CEP/endereço → ponto) para usar a proximidade.
5. Comparação com a média do estado e de municípios semelhantes.
6. Definir e documentar uma metodologia antes de qualquer "CrabCrawler Security Score".
