# APIs de parceiros

**Nenhuma API real está integrada.** Existe só a infraestrutura genérica em
`backend/crates/crawler/src/property_sources/api.rs`, testada contra um
servidor local.

## Infraestrutura pronta

- `ApiClient::get_json`: https obrigatório (http só em loopback), timeout,
  retry com backoff exponencial e limite de tentativas, intervalo mínimo
  entre requisições, `429` respeitando `Retry-After` (em segundos) até um teto,
  depois falha com `RateLimited` (sem retry infinito), limite de tamanho da resposta.
- `ApiAuth`: `None`, `BearerFromEnv(VAR)`, `HeaderFromEnv { header, var }`.
  O segredo é lido da variável de ambiente na hora da requisição; nunca vai
  para o banco nem para logs (URLs são redigidas).
- `ApiMapping`: traduz a resposta da API para `RawProperty`. É a única parte
  específica de cada parceiro.

## Antes de integrar uma API

1. Documentação pública ou contrato do parceiro, com os endpoints reais.
2. Permissão escrita para o uso (análise, exibição, retenção).
3. Limites de requisição e forma de autenticação.
4. Criar `docs/property-sources/<PARCEIRO>.md` com links, data da conferência,
   campos mapeados e semântica (busca, por id, incremental).

A CLI recusa `partner add --provider API` até que isso exista.
