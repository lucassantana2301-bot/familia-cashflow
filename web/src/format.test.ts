import { describe, expect, it } from 'vitest';
import { bpPercent, monthLabel, parseCents, shiftMonth } from './format';

describe('parseCents', () => {
  it.each([
    ['1.234,56', 123456],
    ['45,9', 4590],
    ['45.90', 4590],
    ['R$ 10', 1000],
    ['0,05', 5],
  ])('%s -> %i', (input, cents) => {
    expect(parseCents(input)).toBe(cents);
  });

  it.each(['', 'abc', '-10', '10,999', '1,2,3'])('rejeita %s', (input) => {
    expect(parseCents(input)).toBeNull();
  });
});

describe('meses', () => {
  it('navega entre anos', () => {
    expect(shiftMonth('2026-12', 1)).toBe('2027-01');
    expect(shiftMonth('2026-01', -1)).toBe('2025-12');
    expect(monthLabel('2026-11')).toBe('novembro de 2026');
  });

  it('formata pontos-base', () => {
    expect(bpPercent(8870)).toBe('89%');
    expect(bpPercent(null)).toBe('—');
  });
});
