import { describe, expect, it } from 'vitest';
import { renderToStaticMarkup } from 'react-dom/server';
import { RepositoryLink, RepositoryStarBadge } from '../src/settings/pages/AboutPage';

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

  it('仓库标题、徽章和提醒位于同一个按钮内', () => {
    const html = renderToStaticMarkup(<RepositoryLink stars={3} onOpen={() => {}} />);
    expect(html.match(/<button\b/g)).toHaveLength(1);
    expect(html).toContain('GitHub 仓库');
    expect(html).toContain('aria-label="GitHub Star 3"');
    expect(html).toContain('觉得好用？给项目点个 Star 支持一下');
    expect(html.indexOf('GitHub 仓库')).toBeLessThan(html.indexOf('觉得好用？'));
  });
});
