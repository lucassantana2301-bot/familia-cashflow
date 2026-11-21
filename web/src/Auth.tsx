import { type FormEvent, useState } from 'react';
import { ApiError, api, session } from './api';

export function Auth({ onSignedIn }: { onSignedIn: () => void }) {
  const [mode, setMode] = useState<'login' | 'register'>('login');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const data = new FormData(event.currentTarget);
    const field = (k: string) => String(data.get(k) ?? '');
    setBusy(true);
    setError(null);
    try {
      const { token } =
        mode === 'login'
          ? await api.login({ email: field('email'), password: field('password') })
          : await api.register({
              familyName: field('familyName'),
              name: field('name'),
              email: field('email'),
              password: field('password'),
            });
      session.set(token);
      onSignedIn();
    } catch (e) {
      setError(e instanceof ApiError ? e.message : 'Sem conexão com o servidor. Tente de novo em instantes.');
    } finally {
      setBusy(false);
    }
  }

  return (
    <main className="auth">
      <div className="auth__intro">
        <h1 className="auth__title">O dinheiro da casa, em envelopes.</h1>
        <p className="auth__lead">
          Cada categoria ganha um envelope com o valor do mês. Você importa o extrato do banco e vê quais estão enchendo.
        </p>
      </div>
      <form className="auth__form" onSubmit={submit}>
        <h2>{mode === 'login' ? 'Entrar' : 'Criar a conta da família'}</h2>
        {mode === 'register' && (
          <>
            <label>
              Nome da família
              <input name="familyName" required maxLength={80} placeholder="Família Santana" />
            </label>
            <label>
              Seu nome
              <input name="name" required maxLength={80} autoComplete="name" />
            </label>
          </>
        )}
        <label>
          E-mail
          <input name="email" type="email" required autoComplete="email" />
        </label>
        <label>
          Senha
          <input
            name="password"
            type="password"
            required
            minLength={mode === 'register' ? 10 : 1}
            autoComplete={mode === 'login' ? 'current-password' : 'new-password'}
          />
          {mode === 'register' && <span className="hint">Pelo menos 10 caracteres.</span>}
        </label>
        {error && (
          <p className="form-error" role="alert">
            {error}
          </p>
        )}
        <button type="submit" disabled={busy}>
          {busy ? 'Aguarde…' : mode === 'login' ? 'Entrar' : 'Criar conta'}
        </button>
        <button type="button" className="link" onClick={() => setMode(mode === 'login' ? 'register' : 'login')}>
          {mode === 'login' ? 'Primeira vez? Criar a conta da família' : 'Já tenho conta'}
        </button>
      </form>
    </main>
  );
}
