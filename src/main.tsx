import React from 'react';
import ReactDOM from 'react-dom/client';
import App from './App';
import './styles/index.css';
import { AppProvider } from './contexts/AppContext';

// Disable default right-click context menu for app-like feel
document.addEventListener('contextmenu', (event) => {
  event.preventDefault();
});

// Disable Browser Default Shortcuts (F7 Caret, Search, Refresh, etc.)
window.addEventListener('keydown', (e) => {
    // F7: Caret Browsing (Windows WebView2 default)
    if (e.key === 'F7') {
        e.preventDefault();
        return;
    }
    
    // F3 / Ctrl+F / Ctrl+G: Find
    if (e.key === 'F3' || (e.ctrlKey && (e.key === 'f' || e.key === 'g' || e.key === 'F'))) {
        e.preventDefault();
        return;
    }

    // F5 / Ctrl+R: Refresh
    if (e.key === 'F5' || (e.ctrlKey && (e.key === 'r' || e.key === 'R'))) {
        e.preventDefault();
        return;
    }
    
    // Ctrl+P: Print
    if (e.ctrlKey && (e.key === 'p' || e.key === 'P')) {
        e.preventDefault();
        return;
    }
});

ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
  <React.StrictMode>
    <AppProvider>
      <App />
    </AppProvider>
  </React.StrictMode>
);