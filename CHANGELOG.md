# Changelog

Todas as mudanças relevantes deste projeto são documentadas aqui.

O formato segue o [Keep a Changelog](https://keepachangelog.com/pt-BR/1.1.0/) e o projeto adota o
[Versionamento Semântico](https://semver.org/lang/pt-BR/).

## [Não lançado]

## [1.0.0] - 2026-11-22

Primeira versão estável.

### Adicionado

- **Família com vários membros e papéis**: cada gasto registra quem gastou.
- **Envelopes por categoria** com limite mensal, alerta a partir de 80% e marcação de estouro.
- **Importação de extrato** OFX (1.x SGML e 2.x XML) e CSV no padrão dos bancos brasileiros, com
  deduplicação por `FITID` ou hash estável da linha.
- **Categorização automática por regras** com prioridade, sem diferenciar maiúsculas e acentos.
- **Metas de economia** com aportes e resgates atômicos.
- **Relatório do mês**: entradas, saídas, taxa de poupança, gasto por membro e maiores despesas.
- **Segurança**: isolamento entre famílias garantido por FK composta no banco, sessão opaca revogável
  com Argon2id, login com tempo constante e rate limit.
- **API em Rust** (axum + sqlx) com migrations embutidas e **painel React**.
- **Imagens Docker** (API distroless e web) publicadas no GHCR com SBOM e atestado de proveniência.
- **Qualidade**: CI com `cargo fmt`, `clippy -D warnings`, testes de unidade e de integração com
  PostgreSQL real, e testes do front.

[Não lançado]: https://github.com/lucassantana2301-bot/familia-cashflow/compare/v1.0.0...HEAD
[1.0.0]: https://github.com/lucassantana2301-bot/familia-cashflow/releases/tag/v1.0.0
