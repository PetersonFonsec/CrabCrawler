# VRSync (feed XML de parceiro)

Formato de feed do Grupo ZAP, usado por CRMs de imobiliárias. Só importamos
feeds que **o parceiro nos entrega e autoriza**. Nada aqui acessa portais.

Especificação conferida em 2026-10-03:
<https://developers.grupozap.com/feeds/vrsync/> (Header, Listing, Details,
Exemplos) e as páginas de regras de integração.

## O que o importador lê

Raiz `ListingDataFeed` (namespace `http://www.vivareal.com/schemas/1.0/VRSync`)
→ `Listings/Listing`. Por anúncio:

| VRSync | Uso |
|---|---|
| `ListingID` | id externo (1 a 50 caracteres). Duplicado no mesmo feed: o segundo vira inválido |
| `TransactionType` | `For Sale`, `For Rent`, `Sale/Rent` (gera dois anúncios) |
| `Title`, `Details/Description` | texto, limpo de espaços |
| `DetailViewUrl` | `source_url`, só referência |
| `Details/PropertyType` | ex.: `Residential / Apartment` → `apartment` |
| `Details/ListPrice`, `Details/RentalPrice` | preço; só `currency="BRL"` |
| `Details/PropertyAdministrationFee` | condomínio |
| `Details/Iptu` (ou `YearlyTax`, que aparece no exemplo oficial) | IPTU; `period="Monthly"` é multiplicado por 12 |
| `Details/LivingArea`, `LotArea` | `unit="square metres"` (hectares convertidos) |
| `Bedrooms`, `Bathrooms`, `Suites`, `Garage` | contagens |
| `Location` | endereço, CEP, cidade, UF, lat/lon (`PROPERTY_SOURCE`) |
| `Location@displayAddress` | `All`: tudo; `Street`: sem número/complemento/coordenada; `Neighborhood`: também sem rua |
| `Media/Item` | URLs de imagem (http/https), guardadas, nunca baixadas |

`ContactInfo` **não** é guardado no `raw_payload`. Campos fora da tabela
ficam no `raw_payload` sem interpretação; não inventamos campos.

## Semântica de sincronização

A documentação diz que "alterações, inclusões e exclusões serão refletidas",
mas não afirma que todo arquivo é o estoque completo. Tratamos o feed como
**completo** (`BULK_IMPORT`), com a trava de inativação em massa descrita em
[ARCHITECTURE.md](ARCHITECTURE.md). **Confirmar com cada parceiro.**

- Idempotente: reimportar o mesmo arquivo dá `unchanged` em tudo.
- Item ruim (sem id, moeda estrangeira, CEP inválido no lugar da âncora) vira
  `invalid` com o motivo; o resto do feed segue.
- Anúncio que sumiu → `INACTIVE`; reaparece → `ACTIVE`.

## Segurança do XML

Leitor em streaming (`quick-xml`), em `property_sources/xml.rs`:

- `DOCTYPE` rejeitado (sem DTD, sem XXE, sem entity expansion).
- Só entidades predefinidas e numéricas.
- Limites de profundidade, número de elementos, tamanho de texto, 200 MB por
  arquivo e 50 mil anúncios (o limite do próprio VRSync).
- Download só https (http apenas em loopback), sem credencial na URL,
  com timeout, retry com backoff e tamanho máximo.
- Valores absurdos (preço, área, quantidades) são descartados com aviso.

## Uso

```bash
# parceiro: a credencial fica numa variável de ambiente; no banco só o NOME dela
export PARCEIRO_A_FEED_AUTH='Bearer …'
cargo run -- partner add --slug parceiro-a --name "Imobiliária A" --type REAL_ESTATE_AGENCY \
  --feed-url https://parceiro.example/vrsync.xml --authorization-env PARCEIRO_A_FEED_AUTH
cargo run -- sync properties --partner parceiro-a
cargo run -- sync-runs --limit 5

# arquivo local, sem parceiro (desenvolvimento)
cargo run -- sync vrsync --file fixtures/vrsync/feed_v1.xml
```

## Fixtures

`backend/fixtures/vrsync/` tem feeds **fictícios** escritos a partir da
especificação: `feed_v1.xml`, `feed_v2.xml` (preço alterado, item novo, item
removido), `feed_malformed.xml` e `feed_xxe.xml`. Nenhum é de parceiro real.
