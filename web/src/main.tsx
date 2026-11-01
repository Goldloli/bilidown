import { createRoot } from 'react-dom/client';
import './styles/app.css';
import { App } from './App';
import { bootstrapApp } from './lib/bootstrap';
import { devParams } from './dev/scenario';
import { applyTheme, readThemeOverride, systemTheme } from './lib/theme';
import { applyGlass, readGlassPref } from './lib/glass';
import { applyAccent, readAccentPref } from './lib/accent';
import { applyMotion, readMotionPref, watchSystemMotion } from './lib/motion';

const params = devParams();
if (params.qa) {
  document.documentElement.dataset.qa = 'screenshot';
}
applyTheme(readThemeOverride() ?? systemTheme());
applyGlass(readGlassPref());
applyAccent(readAccentPref());
const initialMotion = readMotionPref();
applyMotion(initialMotion);
watchSystemMotion(initialMotion, () => applyMotion(initialMotion));

const container = document.getElementById('root');

async function start() {
  try {
    await bootstrapApp();
  } catch (error) {
    console.error('[bilidown] bootstrap failed', error);
    if (container) {
      container.innerHTML =
        '<div style="padding:40px;font-family:-apple-system,sans-serif;font-size:13px;color:#b3463f">初始化失败：' +
        String(error instanceof Error ? error.message : error) +
        '</div>';
    }
    return;
  }
  if (container) {
    createRoot(container).render(<App />);
  }
}

void start();
