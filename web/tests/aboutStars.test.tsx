import { describe, expect, it } from 'vitest';
import { renderToStaticMarkup } from 'react-dom/server';
import { RepositoryStarBadge } from '../src/settings/pages/AboutPage';

describe('RepositoryStarBadge（关于页 Star 徽章）', () => {
  it('渲染 Star 数与无障碍标签', () => {
    const html = renderToStaticMarkup(<RepositoryStarBadge stars={128} />);
    expect(html).toContain('128');
    expect(html).toContain('aria-label="GitHub Star 128"');
  });

  it('大于 1000 时紧凑格式化', () => {
    const html = renderToStaticMarkup(<RepositoryStarBadge stars={1200} />);
    expect(html).toContain('1.2k');
  });

  it('stars 为 null 时不渲染（静默降级）', () => {
    const html = renderToStaticMarkup(<RepositoryStarBadge stars={null} />);
    expect(html).toBe('');
  });
});
