# Endereço e coordenada

Separados dos providers: rodam no `PropertyIngestor`, para qualquer fonte.

## CEP → endereço (ViaCEP)

`ADDRESS_LOOKUP=viacep` (padrão `none`). Consulta `https://viacep.com.br/ws/{cep}/json/`.
Preenche só o que falta; divergência de cidade/UF vira aviso. `{"erro": "true"}`
vira aviso "CEP não encontrado". O ViaCEP avisa que uso em massa pode ser
bloqueado: não use em sincronizações grandes sem cache próprio.

## Endereço → coordenada (Nominatim)

`GEOCODER=nominatim` (padrão `none`), com `NOMINATIM_EMAIL` de contato (recomendado pela política). Segue a política
de uso do OSM: no máximo 1 requisição/s (`GEOCODER_MIN_INTERVAL_MS`, padrão
1100; use 15000 em sincronizações recorrentes, que a política limita a 4/min), User-Agent identificável, cache de todo resultado (inclusive "não
encontrado") em `geocoding_cache`, nenhum dado pessoal na consulta. Exige
atribuição "© OpenStreetMap contributors" onde a coordenada for exibida.

Precisão vem do `place_rank`: casa/edifício = `EXACT`, rua = `STREET`,
CEP = `POSTAL_CODE`, resto = `APPROXIMATE`.

## Regras

- Coordenada da fonte (VRSync com `displayAddress=All`) tem prioridade:
  `PROPERTY_SOURCE`, `EXACT`.
- Coordenada digitada: `MANUAL`, `REPORTED`.
- Endereço igual ao gravado: a coordenada existente é mantida, sem nova consulta.
- Falha do serviço não impede a gravação; o imóvel fica sem coordenada.
- A Regional Intelligence só analisa o entorno com precisão `EXACT`,
  `STREET` ou `REPORTED`.
