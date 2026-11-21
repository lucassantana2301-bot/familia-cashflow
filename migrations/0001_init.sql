-- familia-cashflow: esquema inicial (PostgreSQL 15+, por causa de ON DELETE SET NULL (coluna))
-- Dinheiro em centavos (BIGINT). Isolamento entre famílias garantido no banco:
-- toda tabela filha referencia (family_id, id) do pai com FK composta, então é
-- impossível lançar numa conta ou categoria de outra família, mesmo com bug na API.

CREATE TABLE families (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name       TEXT NOT NULL CHECK (char_length(name) BETWEEN 1 AND 80),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE users (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    email         TEXT NOT NULL UNIQUE CHECK (email = lower(email) AND position('@' IN email) > 1),
    name          TEXT NOT NULL CHECK (char_length(name) BETWEEN 1 AND 80),
    password_hash TEXT NOT NULL,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Uma família por usuário (UNIQUE em user_id) simplifica o escopo de todas as consultas.
CREATE TABLE memberships (
    family_id UUID NOT NULL REFERENCES families (id) ON DELETE CASCADE,
    user_id   UUID NOT NULL UNIQUE REFERENCES users (id) ON DELETE CASCADE,
    role      TEXT NOT NULL CHECK (role IN ('OWNER', 'MEMBER')),
    PRIMARY KEY (family_id, user_id)
);

-- Guardamos só o SHA-256 do token: vazamento do banco não expõe sessões válidas.
CREATE TABLE sessions (
    token_hash BYTEA PRIMARY KEY,
    user_id    UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX sessions_user_idx ON sessions (user_id);

CREATE TABLE accounts (
    id         UUID NOT NULL DEFAULT gen_random_uuid(),
    family_id  UUID NOT NULL REFERENCES families (id) ON DELETE CASCADE,
    name       TEXT NOT NULL CHECK (char_length(name) BETWEEN 1 AND 60),
    kind       TEXT NOT NULL CHECK (kind IN ('CHECKING', 'SAVINGS', 'CREDIT_CARD', 'CASH')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (id),
    UNIQUE (family_id, id),
    UNIQUE (family_id, name)
);

CREATE TABLE categories (
    id         UUID NOT NULL DEFAULT gen_random_uuid(),
    family_id  UUID NOT NULL REFERENCES families (id) ON DELETE CASCADE,
    name       TEXT NOT NULL CHECK (char_length(name) BETWEEN 1 AND 40),
    kind       TEXT NOT NULL CHECK (kind IN ('EXPENSE', 'INCOME')),
    PRIMARY KEY (id),
    UNIQUE (family_id, id),
    UNIQUE (family_id, name)
);

CREATE TABLE transactions (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    family_id    UUID NOT NULL,
    account_id   UUID NOT NULL,
    category_id  UUID,
    member_id    UUID REFERENCES users (id) ON DELETE SET NULL,
    amount_cents BIGINT NOT NULL CHECK (amount_cents <> 0),
    description  TEXT NOT NULL CHECK (char_length(description) BETWEEN 1 AND 200),
    occurred_on  DATE NOT NULL,
    -- FITID do OFX ou hash estável da linha do CSV: reimportar o mesmo extrato não duplica.
    external_id  TEXT,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    FOREIGN KEY (family_id, account_id) REFERENCES accounts (family_id, id) ON DELETE CASCADE,
    FOREIGN KEY (family_id, category_id) REFERENCES categories (family_id, id) ON DELETE SET NULL (category_id),
    UNIQUE (account_id, external_id)
);
CREATE INDEX transactions_family_date_idx ON transactions (family_id, occurred_on);

CREATE TABLE budgets (
    family_id   UUID NOT NULL,
    category_id UUID NOT NULL,
    month       DATE NOT NULL CHECK (extract(DAY FROM month) = 1),
    limit_cents BIGINT NOT NULL CHECK (limit_cents > 0),
    PRIMARY KEY (family_id, category_id, month),
    FOREIGN KEY (family_id, category_id) REFERENCES categories (family_id, id) ON DELETE CASCADE
);

CREATE TABLE goals (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    family_id    UUID NOT NULL REFERENCES families (id) ON DELETE CASCADE,
    name         TEXT NOT NULL CHECK (char_length(name) BETWEEN 1 AND 60),
    target_cents BIGINT NOT NULL CHECK (target_cents > 0),
    saved_cents  BIGINT NOT NULL DEFAULT 0 CHECK (saved_cents >= 0),
    target_date  DATE,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE rules (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    family_id   UUID NOT NULL,
    pattern     TEXT NOT NULL CHECK (char_length(pattern) BETWEEN 2 AND 60),
    category_id UUID NOT NULL,
    priority    INTEGER NOT NULL DEFAULT 0,
    FOREIGN KEY (family_id, category_id) REFERENCES categories (family_id, id) ON DELETE CASCADE,
    UNIQUE (family_id, pattern)
);
