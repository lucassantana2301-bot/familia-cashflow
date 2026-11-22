# Como contribuir

Obrigado pelo interesse! Este guia descreve o fluxo usado no projeto.

## Ambiente

Pré-requisitos: Rust 1.80+, Node.js 22+ e Docker (para o PostgreSQL).

```bash
cp .env.example .env              # defina POSTGRES_PASSWORD
docker compose up -d postgres
export DATABASE_URL=postgres://familia:<senha>@localhost:5432/familia
cargo run                          # API em http://localhost:8080
cd web && npm ci && npm run dev    # painel em http://localhost:5173
```

## Fluxo de trabalho

1. Abra (ou escolha) uma issue descrevendo o problema. Mudanças grandes merecem discussão antes do código.
2. Crie uma branch a partir de `main` com prefixo do tipo: `feat/`, `fix/`, `docs/`, `refactor/`, `ci/`, `chore/`.
3. Faça commits pequenos no padrão [Conventional Commits](https://www.conventionalcommits.org/pt-br/v1.0.0/):
   `feat(api): adiciona filtro por região`, `fix(ingest): trata BOM no CSV`.
4. Rode as verificações antes de abrir o PR:

   ```bash
   cargo fmt --check
   cargo clippy --all-targets --locked -- -D warnings
   cargo test --locked
   (cd web && npm run typecheck && npm test && npm run build)
   ```

5. Abra o PR usando o template. O CI precisa estar verde e o PR é integrado com **squash merge**.

## Padrões

- Testes acompanham toda mudança de comportamento; bugs corrigidos ganham um teste de regressão.
- Decisões de arquitetura relevantes viram um ADR em `docs/adr/`.
- Nunca commite segredos, `.env` ou dados reais (billing, clientes, documentos internos).
- Versionamento segue [SemVer](https://semver.org/lang/pt-BR/) e o histórico fica no [CHANGELOG](CHANGELOG.md).

## Release

1. Em um PR, mova os itens de `[Não lançado]` do `CHANGELOG.md` para a nova versão e atualize
   a versão do projeto (`version` no `Cargo.toml` e no `web/package.json`).
2. Ao integrar o PR na `main`, o workflow `release.yml` percebe a versão nova, valida o código,
   publica os artefatos e cria a tag `vX.Y.Z` e a release com as notas do CHANGELOG.
3. Também é possível disparar pela aba Actions (`workflow_dispatch`) ou enviando a tag à mão.
