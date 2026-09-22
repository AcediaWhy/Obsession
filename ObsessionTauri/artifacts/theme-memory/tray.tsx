import React from 'react';
import { createRoot } from 'react-dom/client';
import { HeroField } from '../../src/design/components/HeroField';
import { ThemePreview } from '../../src/design/components/ThemePreview';
import { setWindowShown } from '../../src/design/render';
import { initTrayStageRelease } from '../../src/design/gl/trayStageRelease';
import type { Theme } from '../../src/store/themeStore';
import '../../src/styles/globals.css';

// Isolated WebView diagnostic. No DPI commands, user settings, or service access.
const root = createRoot(document.querySelector('#root')!);
initTrayStageRelease();
let ticks = 0;
setInterval(() => { document.documentElement.dataset.ticks = String(++ticks); }, 250);
function render(theme: Theme) {
  root.render(<div data-theme={theme} style={{ position: 'absolute', inset: 0 }}>
    <HeroField theme={theme} />
    <div style={{ position: 'absolute', left: 20, top: 20 }}><ThemePreview theme={theme} size={240} /></div>
  </div>);
}
Object.assign(window, { trayProbe: { render, shown: setWindowShown } });
render('ophanim');
