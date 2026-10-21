import React from 'react';
import { createRoot } from 'react-dom/client';
import App from './App';
import Overlay from './Overlay';
import './style.css';
import { applyTheme, readTheme } from './theme';

applyTheme(readTheme());

// The overlay window shares this bundle. It is button-only and never takes
// keyboard focus, so it renders its own small surface.
const overlay = new URLSearchParams(window.location.search).has('overlay');

createRoot(document.getElementById('root')!).render(
  <React.StrictMode>{overlay ? <Overlay /> : <App />}</React.StrictMode>);
