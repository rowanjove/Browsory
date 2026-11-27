import React from 'react';
import ReactDOM from 'react-dom/client';
import App from './App';
import './index.css';
// Disable default browser context menu in desktop app
document.addEventListener('contextmenu', (e) => {
  const target = e.target;
  if (target instanceof Element && target.closest('input, textarea, [contenteditable]:not([contenteditable="false"])')) {
    return;
  }
  e.preventDefault();
});

ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
);
