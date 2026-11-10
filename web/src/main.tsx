import { createRoot } from 'react-dom/client';
import { useEffect, useState } from 'react';
import './styles/app.css';
import { App } from './App';
import { BootError } from './components/BootError';
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
// 主题/外观在首帧前应用，避免白闪；网络与后端准备不阻塞首屏（tasks 5.1）。
applyTheme(readThemeOverride() ?? systemTheme());
applyGlass(readGlassPref());
applyAccent(readAccentPref());
const initialMotion = readMotionPref();
applyMotion(initialMotion);
watchSystemMotion(initialMotion, () => applyMotion(initialMotion));

function Boot() {
  const [error, setError] = useState<string | null>(null);
  const [attempt, setAttempt] = useState(0);

  useEffect(() => {
    let alive = true;
    bootstrapApp().then(
      () => {
        if (alive) setError(null);
      },
      (reason: unknown) => {
        if (!alive) return;
        setError(reason instanceof Error ? reason.message : String(reason));
      },
    );
    return () => {
      alive = false;
    };
  }, [attempt]);

  if (error) {
    return (
      <BootError
        message={error}
        onRetry={() => {
          setError(null);
          setAttempt((value) => value + 1);
        }}
      />
    );
  }
  return <App />;
}

const container = document.getElementById('root');
if (container) {
  // 先渲染界面骨架（本地内容可用），bootstrap 在后台完成事件订阅与数据加载。
  createRoot(container).render(<Boot />);
}
