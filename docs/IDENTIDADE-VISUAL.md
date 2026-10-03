# Identidade visual

Toda página, relatório ou visualização do CrabCrawler segue a identidade da
landing page. A fonte da verdade é [`src/styles.scss`](../src/styles.scss) e as
seções em `src/app/pages/landing-page/`. Se os tokens mudarem lá, esta página
deve acompanhar.

## Cores

| Token | Valor | Uso |
|---|---|---|
| `--background-color` | `#f8f1e7` | fundo das páginas (creme) |
| `--neutral-color` | `#faecd8` | blocos de apoio, faixa de fontes |
| `--dark-color` | `#1e1e1e` | texto e seções escuras |
| `--dark-color--secondary` | `#4d4d4d` | texto secundário, rótulos |
| cards escuros | `#262626` | cards sobre seção escura |
| `--primary-color` | `#fa2f3a` | destaque, palavra em destaque no título, foco |
| `--primary-dark-color` | `#5b061a` | hover de botões primários |
| `--energy-color` | `#fff288` | realce sobre fundo escuro, avisos |
| `--success-color` | `#06d6a0` | estado positivo, selo "dado oficial" |
| `--sea-color` | `#3a86c8` | cor de apoio (mapas, séries extras) |
| `--line-color` | `rgba(30,30,30,.14)` | bordas finas |

Tema único claro, como a landing. Não há modo escuro separado: o contraste vem
da alternância entre seções creme e seções escuras.

## Tipografia

- Títulos: **Bricolage Grotesque**, peso 700, `letter-spacing: -0.03em`.
- Texto: **Nunito**, 18px, `line-height: 1.5`.
- Números grandes: Bricolage Grotesque 700, `letter-spacing: -0.05em`,
  `font-variant-numeric: tabular-nums`.
- Rótulos (eyebrow): Nunito 13px, peso 700, caixa alta, `letter-spacing: 0.12em`,
  cor `--dark-color--secondary`.

```html
<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Bricolage+Grotesque:opsz,wght@12..96,400..800&family=Nunito:ital,wght@0,400..900;1,400..900&display=swap">
```

## Formas e espaçamento

- Pílulas e chips: `border-radius: 999px`, peso 700.
- Quadros: `--radius-lg` (32px). Cards: 28px.
- Seções escuras: fundo `--dark-color` com cantos superiores arredondados em 32px.
- Listas (fontes, comparações): linhas separadas por borda de 1.5px `--dark-color`.
- Container: `max-width` de até 1448px e `padding-inline: clamp(20px, 4vw, 56px)`.
- Espaço entre seções: `--section-spacing`, `clamp(96px, 14vw, 200px)` na landing;
  relatórios podem usar uma versão menor.
- Foco: `outline: 3px solid var(--primary-color)`.

## Relatórios

- Cada bloco indica a origem dos números com um selo: "dado oficial" (verde) ou
  "exemplo" (amarelo).
- Nada de nota ou ranking de segurança; ver [SEGURANCA.md](SEGURANCA.md).
