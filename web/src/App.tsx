import { useEffect, useState } from 'react';
import { devParams } from './dev/scenario';
import { GalleryApp } from './dev/GalleryApp';
import { MainWindow } from './shell/MainWindow';
import { PreferencesApp } from './settings/PreferencesApp';

function useHashRoute(): 'main' | 'preferences' | 'gallery' {
  const [hash, setHash] = useState(() => window.location.hash);
  useEffect(() => {
    const onChange = () => setHash(window.location.hash);
    window.addEventListener('hashchange', onChange);
    return () => window.removeEventListener('hashchange', onChange);
  }, []);
  if (hash.startsWith('#/preferences')) return 'preferences';
  if (hash.startsWith('#/gallery')) return 'gallery';
  return 'main';
}

export function App() {
  const route = useHashRoute();
  // 保证 devParams 在首次渲染前已缓存（截图/场景需要）
  void devParams();
  if (route === 'preferences') return <PreferencesApp />;
  if (route === 'gallery') return <GalleryApp />;
  return <MainWindow />;
}
