# Cadastro manual

Tela **Analisar um imóvel** (`/analisar`) e `POST /properties/manual`.

## Campos mínimos

1. **Finalidade**: `sale` ou `rent`.
2. **Preço** em reais (venda ou aluguel mensal), maior que zero.
3. **Localização**: CEP **ou** cidade + UF **ou** latitude + longitude.

Sem um dos três a API responde 422 com o campo que falta. Todo o resto é
opcional: `property_type`, `street`, `number`, `complement`, `neighborhood`,
`area_m2`, `bedrooms`, `bathrooms`, `suites`, `parking_spaces`,
`condominium_fee` (mensal), `property_tax` (IPTU anual), `source_url`,
`notes`, `latitude`/`longitude`.

## Regras

- `source_url` é guardada **só como referência**: http/https, com domínio,
  sem usuário/senha. O backend nunca acessa essa URL. Na tela, o link abre
  com `rel="noopener noreferrer nofollow"`.
- No cadastro manual, valores inválidos (CEP malformado, UF inexistente,
  área absurda) são **erros**; em feeds, os mesmos problemas viram avisos e o
  campo fica vazio.
- Coordenada informada pelo usuário: `coordinate_source = MANUAL`,
  precisão `REPORTED`.
- Com `ADDRESS_LOOKUP=viacep`, o CEP completa rua, bairro, cidade e UF que
  faltarem. Se a cidade informada divergir do CEP, mantemos o informado e
  devolvemos um aviso.
- Cada cadastro gera um imóvel novo (sem id externo, sem deduplicação).

## Resposta

```json
{
  "property": { "id": "…", "municipality": "São Bernardo do Campo", "coordinate_precision": null, … },
  "listing":  { "id": "…", "source": "MANUAL", "transaction": "sale", "status": "ACTIVE", "price_brl": 500000, … },
  "warnings": [],
  "notes": ["Endereço completado pelo CEP (viacep)."],
  "analysis": { "region_ready": false, "municipality_ready": true, "comparison_ready": true,
                "missing": ["Coordenada: informe rua e número (ou ative o geocoding) para analisar a região."] }
}
```

`analysis` diz o que já dá para analisar. A tela mostra isso como chips e,
em seguida, consulta `GET /properties/{id}/region`.

## Fluxo na tela

1. Finalidade, URL (opcional), preço, CEP, endereço, número, tipo, área, quartos, vagas.
2. "Adicionar mais informações" abre complemento, bairro, cidade, UF,
   banheiros, suítes, condomínio, IPTU e observações. Abre sozinho quando
   falta a localização ou a API aponta erro num desses campos.
3. Erros 422 voltam para o campo certo; API fora do ar mostra aviso geral.
4. Resultado: tipo, bairro, preço, chips "Região / Município / Comparação",
   o que falta, observações e resumo da região.

A URL da API vem de `window.CRAB_API_URL` (padrão `http://localhost:3000`).
