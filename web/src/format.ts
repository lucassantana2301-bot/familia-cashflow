const brl = new Intl.NumberFormat('pt-BR', { style: 'currency', currency: 'BRL' });

export const money = (cents: number) => brl.format(cents / 100);

/** Pontos-base (10000 = 100%) para "88%". */
export const bpPercent = (bp: number | null) => (bp === null ? '—' : `${Math.round(bp / 100)}%`);

const MONTHS = ['janeiro', 'fevereiro', 'março', 'abril', 'maio', 'junho', 'julho', 'agosto', 'setembro', 'outubro', 'novembro', 'dezembro'];

export function monthLabel(ym: string): string {
  const [y, m] = ym.split('-').map(Number);
  return `${MONTHS[(m ?? 1) - 1]} de ${y}`;
}

export function shiftMonth(ym: string, delta: number): string {
  const [y, m] = ym.split('-').map(Number);
  const d = new Date(Date.UTC(y ?? 1970, (m ?? 1) - 1 + delta, 1));
  return `${d.getUTCFullYear()}-${String(d.getUTCMonth() + 1).padStart(2, '0')}`;
}

export function currentMonth(now = new Date()): string {
  return new Intl.DateTimeFormat('en-CA', { timeZone: 'America/Sao_Paulo', year: 'numeric', month: '2-digit' }).format(now);
}

export const dayLabel = (iso: string) => `${iso.slice(8, 10)}/${iso.slice(5, 7)}`;

/**
 * "1.234,56" -> 123456; "45.90" -> 4590. Mesma regra do backend:
 * com vírgula, ponto é milhar; sem vírgula, ponto é decimal.
 */
export function parseCents(raw: string): number | null {
  const s = raw.trim().replace(/^R\$\s*/, '');
  if (!s) return null;
  const normalized = s.includes(',') ? s.replace(/\./g, '').replace(',', '.') : s;
  if (!/^\d+(\.\d{1,2})?$/.test(normalized)) return null;
  const [int, frac = ''] = normalized.split('.');
  return Number(int) * 100 + Number(frac.padEnd(2, '0'));
}
