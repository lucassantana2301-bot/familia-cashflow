import { type FormEvent, useCallback, useEffect, useMemo, useState } from 'react';
import {
  type Account,
  ApiError,
  type Category,
  type Envelope,
  type Goal,
  type Me,
  type MonthlyReport,
  type Transaction,
  api,
} from './api';
import { bpPercent, currentMonth, dayLabel, money, monthLabel, parseCents, shiftMonth } from './format';

interface Data {
  me: Me;
  report: MonthlyReport;
  accounts: Account[];
  categories: Category[];
  transactions: Transaction[];
  goals: Goal[];
}

export function Dashboard({ onSignedOut }: { onSignedOut: () => void }) {
  const [month, setMonth] = useState(currentMonth);
  const [data, setData] = useState<Data | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      const [me, report, accounts, categories, transactions, goals] = await Promise.all([
        api.me(),
        api.report(month),
        api.accounts(),
        api.categories(),
        api.transactions(month),
        api.goals(),
      ]);
      setData({ me, report, accounts, categories, transactions, goals });
      setError(null);
    } catch (e) {
      if (e instanceof ApiError && e.status === 401) return onSignedOut();
      setError(e instanceof Error ? e.message : 'Falha ao carregar');
    }
  }, [month, onSignedOut]);

  useEffect(() => {
    void load();
  }, [load]);

  if (error) {
    return (
      <p className="notice notice--error" role="alert">
        {error} <button onClick={() => void load()}>Tentar de novo</button>
      </p>
    );
  }
  if (!data) return <p className="notice">Abrindo os envelopes…</p>;

  const { report } = data;
  const categoryName = new Map(data.categories.map((c) => [c.id, c.name]));

  return (
    <div className="page">
      <header className="topbar">
        <p className="topbar__family">{data.me.familyName}</p>
        <nav className="month" aria-label="Mês">
          <button onClick={() => setMonth(shiftMonth(month, -1))} aria-label="Mês anterior">
            ‹
          </button>
          <span>{monthLabel(month)}</span>
          <button onClick={() => setMonth(shiftMonth(month, 1))} aria-label="Próximo mês">
            ›
          </button>
        </nav>
        <button
          className="link"
          onClick={async () => {
            await api.logout().catch(() => undefined);
            onSignedOut();
          }}
        >
          Sair
        </button>
      </header>

      <section className="headline" aria-label="Resumo do mês">
        <h1>
          {report.netCents >= 0 ? 'Sobrou ' : 'Faltou '}
          <span className={report.netCents >= 0 ? 'pos' : 'neg'}>{money(Math.abs(report.netCents))}</span> em{' '}
          {monthLabel(month).split(' de ')[0]}.
        </h1>
        <p>
          Entraram {money(report.incomeCents)} e saíram {money(report.expenseCents)}
          {report.savingsRateBp !== null && <>: a família guardou {bpPercent(report.savingsRateBp)} do que ganhou</>}.
        </p>
      </section>

      <section aria-labelledby="envelopes-title">
        <h2 id="envelopes-title">Envelopes</h2>
        {report.envelopes.length === 0 ? (
          <p className="empty">Nenhum gasto ainda neste mês. Importe o extrato ou lance uma despesa.</p>
        ) : (
          <ul className="envelopes">
            {report.envelopes.map((e) => (
              <EnvelopeCard key={e.categoryId ?? 'none'} envelope={e} month={month} onChange={load} />
            ))}
          </ul>
        )}
      </section>

      <div className="columns">
        <section aria-labelledby="tx-title" className="ledger">
          <h2 id="tx-title">Lançamentos</h2>
          <NewTransaction accounts={data.accounts} categories={data.categories} onSaved={load} />
          {data.transactions.length === 0 ? (
            <p className="empty">Sem lançamentos em {monthLabel(month)}.</p>
          ) : (
            <ul className="ledger__list">
              {data.transactions.map((t) => (
                <li key={t.id}>
                  <span className="ledger__day">{dayLabel(t.occurredOn)}</span>
                  <span className="ledger__desc">
                    {t.description}
                    <small>{t.categoryId ? categoryName.get(t.categoryId) : 'Sem categoria'}</small>
                  </span>
                  <span className={t.amountCents < 0 ? 'neg' : 'pos'}>{money(t.amountCents)}</span>
                  <button
                    className="icon"
                    aria-label={`Excluir ${t.description}`}
                    onClick={async () => {
                      await api.deleteTransaction(t.id);
                      void load();
                    }}
                  >
                    ×
                  </button>
                </li>
              ))}
            </ul>
          )}
        </section>

        <aside className="side">
          <ImportStatement accounts={data.accounts} onImported={load} />
          <Goals goals={data.goals} onChange={load} />
          {report.members.length > 1 && (
            <section aria-labelledby="members-title">
              <h2 id="members-title">Quem gastou</h2>
              <ul className="plain">
                {report.members.map((m) => (
                  <li key={m.memberId ?? 'shared'}>
                    {m.name} <strong>{money(m.spentCents)}</strong>
                  </li>
                ))}
              </ul>
            </section>
          )}
        </aside>
      </div>
    </div>
  );
}

const STATUS_TEXT: Record<Envelope['status'], string> = {
  NO_BUDGET: 'sem limite definido',
  OK: 'dentro do combinado',
  WARNING: 'quase no limite',
  OVER: 'estourou',
};

function EnvelopeCard({ envelope: e, month, onChange }: { envelope: Envelope; month: string; onChange: () => void }) {
  const [editing, setEditing] = useState(false);
  const fill = e.usedBp === null ? 0 : Math.min(e.usedBp / 10000, 1);

  async function saveLimit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const cents = parseCents(String(new FormData(event.currentTarget).get('limit') ?? ''));
    if (!cents || !e.categoryId) return;
    await api.setBudget({ categoryId: e.categoryId, month, limitCents: cents });
    setEditing(false);
    onChange();
  }

  return (
    <li className={`envelope envelope--${e.status.toLowerCase()}`}>
      <div className="envelope__fill" style={{ transform: `scaleY(${fill})` }} aria-hidden />
      <h3>{e.name}</h3>
      <p className="envelope__spent">{money(e.spentCents)}</p>
      <p className="envelope__meta">
        {e.limitCents !== null ? `de ${money(e.limitCents)}, ${bpPercent(e.usedBp)}` : STATUS_TEXT[e.status]}
      </p>
      {e.status === 'OVER' && <p className="envelope__flag">Estourou</p>}
      {e.categoryId &&
        (editing ? (
          <form onSubmit={saveLimit} className="envelope__form">
            <label className="visually-hidden" htmlFor={`limit-${e.categoryId}`}>
              Limite de {e.name}
            </label>
            <input id={`limit-${e.categoryId}`} name="limit" inputMode="decimal" placeholder="800,00" autoFocus />
            <button type="submit">Salvar</button>
          </form>
        ) : (
          <button className="link" onClick={() => setEditing(true)}>
            {e.limitCents === null ? 'Definir limite' : 'Alterar limite'}
          </button>
        ))}
    </li>
  );
}

function NewTransaction({
  accounts,
  categories,
  onSaved,
}: {
  accounts: Account[];
  categories: Category[];
  onSaved: () => void;
}) {
  const [message, setMessage] = useState<string | null>(null);
  const today = useMemo(
    () => new Intl.DateTimeFormat('en-CA', { timeZone: 'America/Sao_Paulo' }).format(new Date()),
    [],
  );

  if (accounts.length === 0) return <NewAccount onSaved={onSaved} />;

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = event.currentTarget;
    const data = new FormData(form);
    const cents = parseCents(String(data.get('amount') ?? ''));
    if (!cents) return setMessage('Valor inválido. Use o formato 45,90.');
    const sign = data.get('direction') === 'in' ? 1 : -1;
    try {
      await api.createTransaction({
        accountId: String(data.get('accountId')),
        categoryId: String(data.get('categoryId')) || null,
        amountCents: sign * cents,
        description: String(data.get('description')),
        occurredOn: String(data.get('occurredOn')),
      });
      form.reset();
      setMessage(null);
      onSaved();
    } catch (e) {
      setMessage(e instanceof ApiError ? e.message : 'Não foi possível salvar.');
    }
  }

  return (
    <form className="quick" onSubmit={submit}>
      <fieldset className="quick__direction">
        <legend className="visually-hidden">Tipo</legend>
        <label>
          <input type="radio" name="direction" value="out" defaultChecked /> Gasto
        </label>
        <label>
          <input type="radio" name="direction" value="in" /> Entrada
        </label>
      </fieldset>
      <input name="description" required maxLength={200} placeholder="Descrição" aria-label="Descrição" />
      <input name="amount" required inputMode="decimal" placeholder="0,00" aria-label="Valor" />
      <select name="categoryId" aria-label="Categoria" defaultValue="">
        <option value="">Sem categoria</option>
        {categories.map((c) => (
          <option key={c.id} value={c.id}>
            {c.name}
          </option>
        ))}
      </select>
      <select name="accountId" aria-label="Conta">
        {accounts.map((a) => (
          <option key={a.id} value={a.id}>
            {a.name}
          </option>
        ))}
      </select>
      <input name="occurredOn" type="date" required defaultValue={today} max={today} aria-label="Data" />
      <button type="submit">Lançar</button>
      {message && (
        <p className="form-error" role="alert">
          {message}
        </p>
      )}
    </form>
  );
}

function NewAccount({ onSaved }: { onSaved: () => void }) {
  return (
    <form
      className="quick"
      onSubmit={async (event) => {
        event.preventDefault();
        const data = new FormData(event.currentTarget);
        await api.createAccount({ name: String(data.get('name')), kind: data.get('kind') as Account['kind'] });
        onSaved();
      }}
    >
      <p className="quick__intro">Para começar, cadastre a conta do banco onde o dinheiro da casa circula.</p>
      <input name="name" required maxLength={60} placeholder="Nubank conjunta" aria-label="Nome da conta" />
      <select name="kind" aria-label="Tipo de conta" defaultValue="CHECKING">
        <option value="CHECKING">Conta corrente</option>
        <option value="SAVINGS">Poupança</option>
        <option value="CREDIT_CARD">Cartão de crédito</option>
        <option value="CASH">Dinheiro</option>
      </select>
      <button type="submit">Cadastrar conta</button>
    </form>
  );
}

function ImportStatement({ accounts, onImported }: { accounts: Account[]; onImported: () => void }) {
  const [status, setStatus] = useState<string | null>(null);
  if (accounts.length === 0) return null;

  return (
    <section aria-labelledby="import-title">
      <h2 id="import-title">Importar extrato</h2>
      <form
        className="stack"
        onSubmit={async (event) => {
          event.preventDefault();
          const data = new FormData(event.currentTarget);
          const file = data.get('file');
          if (!(file instanceof File) || file.size === 0) return setStatus('Escolha um arquivo .ofx ou .csv.');
          setStatus('Lendo o extrato…');
          try {
            const r = await api.importStatement(String(data.get('accountId')), file);
            setStatus(
              `${r.imported} lançamentos importados, ${r.categorized} já categorizados` +
                (r.duplicates ? `, ${r.duplicates} já existiam.` : '.'),
            );
            onImported();
          } catch (e) {
            setStatus(e instanceof ApiError ? e.message : 'Falha no envio.');
          }
        }}
      >
        <select name="accountId" aria-label="Conta do extrato">
          {accounts.map((a) => (
            <option key={a.id} value={a.id}>
              {a.name}
            </option>
          ))}
        </select>
        <input type="file" name="file" accept=".ofx,.csv,.txt" aria-label="Arquivo do extrato" />
        <button type="submit">Importar</button>
        <p className="status" role="status" aria-live="polite">
          {status}
        </p>
      </form>
    </section>
  );
}

function Goals({ goals, onChange }: { goals: Goal[]; onChange: () => void }) {
  return (
    <section aria-labelledby="goals-title">
      <h2 id="goals-title">Metas</h2>
      <ul className="goals">
        {goals.map((g) => {
          const pct = Math.min(g.savedCents / g.targetCents, 1);
          return (
            <li key={g.id}>
              <p className="goals__name">
                {g.name}
                {g.targetDate && <small> até {dayLabel(g.targetDate)}/{g.targetDate.slice(0, 4)}</small>}
              </p>
              <div className="goals__track" role="progressbar" aria-valuenow={Math.round(pct * 100)} aria-valuemin={0} aria-valuemax={100} aria-label={g.name}>
                <span style={{ width: `${pct * 100}%` }} />
              </div>
              <p className="goals__numbers">
                {money(g.savedCents)} de {money(g.targetCents)}
              </p>
              <form
                className="goals__add"
                onSubmit={async (event) => {
                  event.preventDefault();
                  const form = event.currentTarget;
                  const cents = parseCents(String(new FormData(form).get('amount') ?? ''));
                  if (!cents) return;
                  await api.contribute(g.id, cents);
                  form.reset();
                  onChange();
                }}
              >
                <input name="amount" inputMode="decimal" placeholder="Guardar 100,00" aria-label={`Guardar em ${g.name}`} />
                <button type="submit">Guardar</button>
              </form>
            </li>
          );
        })}
      </ul>
      <form
        className="stack"
        onSubmit={async (event) => {
          event.preventDefault();
          const form = event.currentTarget;
          const data = new FormData(form);
          const cents = parseCents(String(data.get('target') ?? ''));
          if (!cents) return;
          await api.createGoal({
            name: String(data.get('name')),
            targetCents: cents,
            targetDate: String(data.get('date')) || null,
          });
          form.reset();
          onChange();
        }}
      >
        <input name="name" required maxLength={60} placeholder="Nova meta, ex.: viagem de férias" aria-label="Nome da meta" />
        <div className="row">
          <input name="target" required inputMode="decimal" placeholder="Valor" aria-label="Valor da meta" />
          <input name="date" type="date" aria-label="Data alvo" />
        </div>
        <button type="submit">Criar meta</button>
      </form>
    </section>
  );
}
