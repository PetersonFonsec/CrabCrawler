# SGB — Setorização de Áreas de Risco Geológico

Última verificação: 2026-10-03 (consulta real ao serviço: 32 setores para São
Bernardo do Campo).

| | |
|---|---|
| URL oficial | <https://geoportal.sgb.gov.br/server/rest/services/gestaoterritorial/risco/MapServer/0> |
| Dataset | Camada 0 "Setorização de Risco" do serviço `gestaoterritorial/risco` |
| Formato | ArcGIS REST `query` com `f=geojson` e `outSR=4326`; até 100 000 registros por consulta (o importador pagina de 1 000 em 1 000) |
| Granularidade | Polígono (setor de risco) |
| Cobertura | Municípios mapeados pelo SGB, consultados pelo código IBGE (`cd_geocmu`) |
| Atualização | Por município, conforme novos mapeamentos. São Bernardo do Campo: mapeamento de 2014-05-14. |

Campos usados: `cd_geocmu`, `num_setor` (identificador original, ex.:
`SP_SBC_SR_08_CPRM`), `local`, `tipolo_g1..5` / `tipolo_e1..5` (tipologia geral
e específica), `cobrade_01..05`, `grau_risco` (ex.: "Alto", "Muito Alto"),
`data_setor` (epoch em ms). Os demais (`grau_vulne`, `num_edif`, `num_domi`,
`num_pess`, `sug_interv`...) ficam em `attributes`.

Tipos de risco seguem a COBRADE: 1.2.1 inundação → `FLOOD`, 1.2.2 enxurrada →
`FLASH_FLOOD`, 1.2.3 alagamento → `URBAN_FLOODING`, 1.1.3.2 deslizamento →
`LANDSLIDE`, outros 1.1.3 (quedas, rastejo, corridas, subsidência) →
`GEOLOGICAL`, 1.1.4 erosão → `EROSION`. O rótulo original é sempre devolvido.
O grau de risco é o texto da fonte; não há escala numérica.

## Ingestão

```bash
cargo run -- crawl sgb-risco --municipality 3548708
cargo run -- crawl sgb-risco --file setorizacao_risco_3548708.geojson
```

A resposta da API é gravada em `data/raw/sgb/` antes do parse. Um município
consultado sem nenhum setor fica registrado como coberto: a API responde
`NOT_IN_MAPPED_RISK_AREA`, nunca "sem risco".

## Limitações

- Pela metodologia do programa, são setores de risco alto e muito alto em
  áreas ocupadas; não é um mapa de suscetibilidade do município inteiro.
- O mapeamento de São Bernardo é de 2014; pode haver áreas novas não mapeadas.
- Estar fora de um setor mapeado não significa ausência de risco.
