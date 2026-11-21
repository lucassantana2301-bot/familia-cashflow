export type EnvelopeStatus = 'NO_BUDGET' | 'OK' | 'WARNING' | 'OVER';

export interface Envelope {
  categoryId: string | null;
  name: string;
  spentCents: number;
  limitCents: number | null;
  usedBp: number | null;
  status: EnvelopeStatus;
}

export interface MonthlyReport {
  month: string;
  incomeCents: number;
  expenseCents: number;
  netCents: number;
  savingsRateBp: number | null;
  envelopes: Envelope[];
  members: { memberId: string | null; name: string; spentCents: number }[];
  topExpenses: { description: string; amountCents: number }[];
}

export interface Account {
  id: string;
  name: string;
  kind: 'CHECKING' | 'SAVINGS' | 'CREDIT_CARD' | 'CASH';
  balanceCents: number;
}
export interface Category {
  id: string;
  name: string;
  kind: 'EXPENSE' | 'INCOME';
}
export interface Transaction {
  id: string;
  accountId: string;
  categoryId: string | null;
  memberId: string | null;
  amountCents: number;
  description: string;
  occurredOn: string;
}
export interface Goal {
  id: string;
  name: string;
  targetCents: number;
  savedCents: number;
  targetDate: string | null;
}
export interface Me {
  userId: string;
  familyId: string;
  familyName: string;
  members: { id: string; name: string; email: string; role: 'OWNER' | 'MEMBER' }[];
}
export interface ImportResult {
  read: number;
  imported: number;
  duplicates: number;
  categorized: number;
}

export class ApiError extends Error {
  constructor(
    public readonly status: number,
    message: string,
  ) {
    super(message);
  }
}

const TOKEN_KEY = 'fc.token';

export const session = {
  get(): string | null {
    try {
      return localStorage.getItem(TOKEN_KEY);
    } catch {
      return null;
    }
  },
  set(token: string) {
    try {
      localStorage.setItem(TOKEN_KEY, token);
    } catch {
      /* sem storage: sessão dura só até recarregar */
    }
  },
  clear() {
    try {
      localStorage.removeItem(TOKEN_KEY);
    } catch {
      /* noop */
    }
  },
};

async function request<T>(path: string, init: RequestInit = {}): Promise<T> {
  const token = session.get();
  const isForm = init.body instanceof FormData;
  const res = await fetch(`/api${path}`, {
    ...init,
    headers: {
      ...(init.body && !isForm ? { 'content-type': 'application/json' } : {}),
      ...(token ? { authorization: `Bearer ${token}` } : {}),
      ...init.headers,
    },
  });
  if (res.status === 204) return undefined as T;
  const body = (await res.json().catch(() => ({}))) as { message?: string };
  if (!res.ok) throw new ApiError(res.status, body.message ?? `Erro ${res.status}`);
  return body as T;
}

const post = <T>(path: string, body: unknown) => request<T>(path, { method: 'POST', body: JSON.stringify(body) });

export const api = {
  register: (b: { familyName: string; name: string; email: string; password: string }) =>
    post<{ token: string }>('/auth/register', b),
  login: (b: { email: string; password: string }) => post<{ token: string }>('/auth/login', b),
  logout: () => request<void>('/auth/logout', { method: 'POST' }),
  me: () => request<Me>('/me'),
  report: (month: string) => request<MonthlyReport>(`/reports/monthly?month=${month}`),
  accounts: () => request<Account[]>('/accounts'),
  createAccount: (b: { name: string; kind: Account['kind'] }) => post<Account>('/accounts', b),
  categories: () => request<Category[]>('/categories'),
  transactions: (month: string) => request<Transaction[]>(`/transactions?month=${month}`),
  createTransaction: (b: {
    accountId: string;
    categoryId: string | null;
    amountCents: number;
    description: string;
    occurredOn: string;
  }) => post<Transaction>('/transactions', b),
  deleteTransaction: (id: string) => request<void>(`/transactions/${id}`, { method: 'DELETE' }),
  setBudget: (b: { categoryId: string; month: string; limitCents: number }) =>
    request('/budgets', { method: 'PUT', body: JSON.stringify(b) }),
  goals: () => request<Goal[]>('/goals'),
  createGoal: (b: { name: string; targetCents: number; targetDate: string | null }) => post<Goal>('/goals', b),
  contribute: (id: string, amountCents: number) => post<Goal>(`/goals/${id}/contributions`, { amountCents }),
  importStatement: (accountId: string, file: File) => {
    const form = new FormData();
    form.set('accountId', accountId);
    form.set('file', file);
    return request<ImportResult>('/imports', { method: 'POST', body: form });
  },
};
