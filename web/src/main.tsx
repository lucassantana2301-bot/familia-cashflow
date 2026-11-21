import { StrictMode, useCallback, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { session } from './api';
import { Auth } from './Auth';
import { Dashboard } from './Dashboard';
import './styles.css';

function App() {
  const [signedIn, setSignedIn] = useState(() => session.get() !== null);
  const signOut = useCallback(() => {
    session.clear();
    setSignedIn(false);
  }, []);
  return signedIn ? <Dashboard onSignedOut={signOut} /> : <Auth onSignedIn={() => setSignedIn(true)} />;
}

const root = document.getElementById('root');
if (!root) throw new Error('#root não encontrado');
createRoot(root).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
