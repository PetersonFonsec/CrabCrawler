# IBGE — Censo Demográfico 2022 por setor censitário

Última verificação: 2026-10-03 (listagem das pastas oficiais lida pela web; o
ambiente de desenvolvimento não consegue baixar os arquivos).

## Malha de setores censitários

| | |
|---|---|
| URL oficial | <https://geoftp.ibge.gov.br/organizacao_do_territorio/malhas_territoriais/malhas_de_setores_censitarios__divisoes_intramunicipais/censo_2022/setores/gpkg/UF/SP/> |
| Dataset | `SP_setores_CD2022.gpkg` (170 MB, publicado em 2024-11-12). Também há `shp/` e `kml/`. |
| Formato | GeoPackage (SQLite). Uma camada de feições. |
| Granularidade | Setor censitário (código de 15 dígitos: UF 2 + município 5 + distrito 2 + subdistrito 2 + setor 4) |
| Cobertura | Brasil, um arquivo por UF |
| Atualização | Decenal (Censo). O IBGE publicou correções pontuais depois do lançamento. |
| Campos usados | `CD_SETOR`, `NM_MUN`, `AREA_KM2`, geometria |
| SRID | Lido de `gpkg_spatial_ref_sys`; esperado SIRGAS 2000 (EPSG:4674). Convertido para 4326 no PostGIS. |

**A conferir no arquivo real:** os nomes `CD_SETOR`, `NM_MUN` e `AREA_KM2`
seguem o dicionário de dados do IBGE, mas o arquivo não pôde ser aberto daqui.
O importador procura as colunas sem diferenciar maiúsculas e falha com a lista
de colunas encontradas se `CD_SETOR` não existir. Códigos com sufixo (ex.:
`...P` das malhas preliminares) são aceitos desde que sobrem 15 dígitos.

## Agregados por setores — arquivo Básico

| | |
|---|---|
| URL oficial | <https://ftp.ibge.gov.br/Censos/Censo_Demografico_2022/Agregados_por_Setores_Censitarios/Agregados_por_Setor_csv/> |
| Dataset | `Agregados_por_setores_basico_BR_20260520.zip` (15 MB, versão de 2026-05-20) |
| Formato | CSV zipado. Separador e codificação são detectados (`;`/`,`, UTF-8/Latin-1). |
| Granularidade | Setor censitário |
| Cobertura | Brasil |
| Atualização | Decenal, com revisões (a versão de 2026-05-20 substituiu a de 2024) |
| Dicionário | `dicionario_de_dados_agregados_por_setores_censitarios_20260520.xlsx` na pasta acima |

Variáveis usadas (nota metodológica n. 06/2024):

| Variável | Significado | Indicador no CrabCrawler |
|---|---|---|
| V0001 | Total de pessoas | `population` |
| V0002 | Total de domicílios | `households` |
| V0003 | Domicílios particulares | `private_households` |
| V0004 | Domicílios coletivos | `collective_households` |
| V0005 | Média de moradores em domicílios particulares ocupados | `avg_residents_per_household` |
| V0007 | Domicílios particulares ocupados | `occupied_private_households` |

V0006 (percentual de domicílios imputados) é qualidade da coleta e não entra.
Valores `X` (sigilo) são gravados sem número, com o marcador em `value_text`, e
a API responde `suppressed_by_source`.

**Densidade populacional** não é publicada no arquivo Básico. A API calcula
`V0001 / AREA_KM2` (área do setor na malha) e marca o valor como
`derived: true`. O IBGE publica também
`Area_efetivamente_domiciliada_e_densidade_ajustada_dos_Setores_Censitarios.xlsx`
(densidade sobre a área domiciliada), ainda não integrado.

**Renda:** a nota metodológica cita `Agregados_por_setores_renda_responsavel`,
mas esse arquivo **não está** na pasta CSV atual (verificado em 2026-10-03).
O perfil responde `average_income: not_published`.

## Ingestão

```bash
cd backend
# baixe os arquivos nas URLs acima, depois:
cargo run -- crawl ibge-setores --file SP_setores_CD2022.gpkg --scope 3548708
cargo run -- crawl ibge-agregados --file Agregados_por_setores_basico_BR_20260520.zip --scope 3548708
```

`--scope` aceita um município (7 dígitos) ou uma UF (2 dígitos, ex.: `35`).
A versão gravada é `<arquivo>@sha256:<prefixo>`: reimportar o mesmo arquivo não
duplica nada; um arquivo novo substitui os setores do recorte.

## Limitações

- Retrato de 2022; não há atualização intercensitária por setor.
- Setores são unidades operacionais da coleta, não bairros.
- Setores muito pequenos podem ter quase todos os valores suprimidos.
