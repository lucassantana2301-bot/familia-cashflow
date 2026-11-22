<div align="center">

# familia-cashflow

**Orçamento da família pelo método dos envelopes: cada categoria tem um limite no mês, o extrato do banco entra por importação e o painel mostra quais envelopes estão enchendo.**

[![CI](https://github.com/lucassantana2301-bot/familia-cashflow/actions/workflows/ci.yml/badge.svg)](https://github.com/lucassantana2301-bot/familia-cashflow/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/lucassantana2301-bot/familia-cashflow?sort=semver)](https://github.com/lucassantana2301-bot/familia-cashflow/releases)
![Rust](https://img.shields.io/badge/Rust-axum_0.8-000000?logo=rust&logoColor=white)
![PostgreSQL](https://img.shields.io/badge/PostgreSQL-16-4169E1?logo=postgresql&logoColor=white)
![React](https://img.shields.io/badge/React-18-61DAFB?logo=react&logoColor=black)
![TypeScript](https://img.shields.io/badge/TypeScript-5.7-3178C6?logo=typescript&logoColor=white)
![Docker](https://img.shields.io/badge/Docker-distroless-2496ED?logo=docker&logoColor=white)
![License](https://img.shields.io/badge/license-MIT-green)

![Painel com o resumo do mês, envelopes por categoria, lançamentos e metas](docs/dashboard.png)

</div>

## O que faz

- **Família com vários membros**: o responsável cria a conta da casa e cadastra cônjuge e filhos. Cada gasto registra quem gastou.
- **Envelopes por categoria**: limite mensal por categoria, com alerta a partir de 80% e marcação de estouro.
- **Importação de extrato** OFX (1.x SGML e 2.x XML) e CSV no padrão dos bancos brasileiros (`;`, `1.234,56`, `dd/mm/aaaa`, Latin-1).
- **Reimportar não duplica**: usa o `FITID` do OFX ou um hash estável da linha do CSV.
- **Categorização automática** por regras ("contém *ifood* → Restaurantes"), sem diferenciar maiúsculas e acentos, com prioridade.
- **Metas de economia** com aportes e resgates atômicos.
- **Relatório do mês**: entradas, saídas, taxa de poupança, gasto por membro e maiores despesas.

## Arquitetura

```mermaid
flowchart LR
    WEB[React + Vite<br/>painel de envelopes] -->|/api via nginx| API

    subgraph API["Rust · axum 0.8 · tokio"]
        direction TB
        MW[Middlewares<br/>CORS · request-id · limites de corpo<br/>headers de segurança · rate limit]
        AUTH[Sessão opaca<br/>Argon2id + SHA-256]
        H[Handlers]
        subgraph PURE["Módulos puros, testados sem I/O"]
            IMP[importer<br/>OFX / CSV]
            CAT[categorize<br/>regras]
            REP[report<br/>envelopes]
            MON[money<br/>centavos i64]
        end
        MW --> AUTH --> H --> PURE
    end

    H -->|sqlx · consultas parametrizadas| PG[(PostgreSQL 16<br/>FKs compostas por família)]
```

### Decisões técnicas

| Decisão | Motivo |
| --- | --- |
| Dinheiro em centavos `i64` / `BIGINT` | Ponto flutuante não fecha o mês. Uso do orçamento em pontos-base inteiros (10000 = 100%). |
| **FK composta `(family_id, id)`** em contas, categorias, regras e orçamentos | O isolamento entre famílias é garantido pelo banco: mesmo com bug na API, é impossível lançar numa conta de outra família. Coberto por teste. |
| `family_id` sempre vem da sessão | Nenhum endpoint aceita `familyId` do cliente; campos desconhecidos no JSON são rejeitados (`deny_unknown_fields`). |
| Sessão opaca em vez de JWT | Revogável no logout. O banco guarda só o SHA-256 do token, então um vazamento do banco não expõe sessões. |
| Argon2id (19 MiB, t=2) em `spawn_blocking` | Parâmetros da OWASP, sem travar o executor async. |
| Login com hash fictício para e-mail inexistente | Mesmo tempo de resposta nos dois casos: evita enumeração de usuários. |
| Rate limit por IP em login/cadastro | 10 tentativas a cada 5 min. `X-Forwarded-For` só é confiável com `TRUST_PROXY=true`. |
| Importação em lote com `UNNEST` + `ON CONFLICT DO NOTHING` | Uma ida ao banco para milhares de linhas e deduplicação atômica. |
| Aporte em meta com `UPDATE ... SET saved = saved + $n` | Sem perda de atualização entre aportes simultâneos; `CHECK (saved_cents >= 0)` barra resgate maior que o saldo. |
| Migrations embutidas no binário (`sqlx::migrate!`) | O container sobe e aplica o esquema sozinho. |
| Imagem final distroless, usuário não-root | Sem shell nem gerenciador de pacotes na superfície de ataque. |

## Como rodar

### Docker

```bash
cp .env.example .env    # defina POSTGRES_PASSWORD
docker compose up --build
```

Painel em http://localhost:8081. Para testar a importação, use `samples/extrato-exemplo.csv`.

### Local

```bash
# API (precisa de um Postgres 15+)
export DATABASE_URL=postgres://familia:senha@localhost:5432/familia
cargo run

# Front
cd web && npm install && npm run dev   # http://localhost:5173
```

## API

Rotas sob `/api`. Exceto cadastro e login, todas exigem `Authorization: Bearer <token>`.

| Método | Rota | Descrição |
| --- | --- | --- |
| `POST` | `/auth/register` | Cria família, responsável e categorias padrão |
| `POST` | `/auth/login` · `/auth/logout` | Abre e revoga sessão |
| `GET` | `/me` | Família e membros |
| `POST` | `/members` | Responsável cadastra um membro |
| `GET` `POST` | `/accounts` | Contas com saldo |
| `GET` `POST` | `/categories` | Categorias de despesa e entrada |
| `GET` `POST` | `/transactions?month=2026-11` | Lançamentos do mês |
| `DELETE` | `/transactions/{id}` | Remove lançamento |
| `POST` | `/imports` | `multipart/form-data` com `accountId` e `file` (.ofx/.csv, até 5 MB) |
| `GET` `POST` `DELETE` | `/rules` | Regras de categorização |
| `GET` `PUT` | `/budgets` | Limite da categoria no mês |
| `GET` `POST` | `/goals` · `POST /goals/{id}/contributions` | Metas e aportes |
| `GET` | `/reports/monthly?month=2026-11` | Resumo, envelopes, membros e maiores gastos |

Erros seguem `{ "error": "CODIGO", "message": "..." }`: `422` validação, `401` sessão, `403` papel, `404` (também para id de outra família, sem revelar que existe), `409` conflito, `429` rate limit e `500` sem detalhes internos.

## Testes

```bash
cargo test                                         # 23 testes de unidade (sem banco)
TEST_DATABASE_URL=postgres://... cargo test        # + 7 testes de integração da API
cd web && npm test                                 # parsing de valores e datas no front
```

Os testes de integração sobem o router real contra o Postgres e cobrem cadastro/login/logout, isolamento entre famílias (ler, apagar e lançar em conta alheia), importação com regras e deduplicação, envelopes no relatório, metas, papéis, rate limit e headers de segurança.

## Stack

**Rust** · **axum** · **tokio** · **sqlx** · **Argon2** · **PostgreSQL** · **React** · **TypeScript** · **Vite** · **Vitest** · **Docker (distroless)** · **nginx** · **GitHub Actions**

## Limitações conhecidas

- Uma família por usuário.
- Transferência entre contas e pagamento de fatura do cartão contam como gasto e entrada: ainda não há tipo "transferência".
- Rate limit em memória: com várias réplicas da API, precisa de um armazenamento compartilhado (Redis).
- O token fica no `localStorage`. A CSP do nginx restringe scripts à própria origem, mas um cookie `HttpOnly` seria mais robusto.

## Roadmap

- [ ] Transferências entre contas e faturas de cartão
- [ ] Lançamentos recorrentes (aluguel, assinaturas)
- [ ] Sugestão de categoria com base no histórico da família
- [ ] Convite de membro por link, sem senha inicial

## Contribuindo

Veja o [guia de contribuição](CONTRIBUTING.md), o [CHANGELOG](CHANGELOG.md) e a [política de segurança](SECURITY.md). Os itens do roadmap estão abertos como [issues](../../issues).

## Licença

MIT © Lucas Santana Santos
