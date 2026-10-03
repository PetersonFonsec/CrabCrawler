# Fundação Seade — IPVS (Índice Paulista de Vulnerabilidade Social)

Última verificação: 2026-10-03 (páginas do Portal de Dados Abertos de SP lidas
pela web; o repositório do Seade não respondeu daqui).

| | |
|---|---|
| URL oficial | <https://dadosabertos.sp.gov.br/dataset/seade-ipvs-versao-2022> |
| Dataset | "Seade IPVS versão 2022" (CSV). O repositório do Seade publica também `ipvs_2022.zip` (shapefile com dicionário). |
| Licença | CC-BY 4.0 |
| Formato | CSV |
| Granularidade | Setor censitário do Censo 2022 |
| Cobertura | Estado de São Paulo |
| Atualização | Decenal (acompanha o Censo). Página atualizada em 2026-01-08. |
| Campos usados | código do setor e grupo do IPVS |

O IPVS classifica setores em grupos de vulnerabilidade a partir de renda e
ciclo de vida das famílias. O CrabCrawler grava o grupo **como a fonte
publica** (`value` = número do grupo, `value_text` = texto original) e não cria
escala própria.

**A conferir no arquivo real:** os nomes das colunas não puderam ser lidos.
O importador detecta a coluna do setor (`CD_SETOR` ou a primeira com "setor")
e a do grupo (a primeira com "ipvs", depois "grupo"). Se a detecção errar:

```bash
cargo run -- crawl seade-ipvs --file ipvs_2022.csv --scope 3548708 \
  --sector-column <coluna_do_setor> --group-column <coluna_do_grupo>
```

O importador recusa recortes fora de SP (`--scope` precisa começar com `35`).

## Limitações

- Classificação relativa entre setores paulistas, não medida de renda.
- Os nomes dos grupos da versão 2022 não foram conferidos; por isso o texto
  original é sempre devolvido.
- Setores sem população suficiente podem ficar sem grupo.
