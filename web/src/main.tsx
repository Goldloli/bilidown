import { createRoot } from 'react-dom/client';
import { useEffect } from 'react';
import './styles/app.css';
import { App } from './App';
import { BootError } from './components/BootError';
import { bootstrapApp } from './lib/bootstrap';
import { useStartupStore } from './stores/useStartupStore';
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

/** 首屏立即渲染骨架：bootstrap 在后台完成订阅与分区加载，不等待网络（tasks 5.1）。 */
function Boot() {
  const fatalError = useStartupStore((s) => s.fatalError);

  useEffect(() => {
    void bootstrapApp();
  }, []);

  if (fatalError) {
    return (
      <BootError
        message={fatalError}
        onRetry={() => {
          useStartupStore.getState().clearFatalError();
          void bootstrapApp();
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
