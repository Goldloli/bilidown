import { describe, expect, it } from 'vitest';
import { renderToStaticMarkup } from 'react-dom/server';
import { Avatar } from '../src/components/Avatar';

describe('Avatar', () => {
  it('uses the user icon when no avatar is available', () => {
    const html = renderToStaticMarkup(<Avatar src={null} className="h-8 w-8" />);
    expect(html).toContain('data-avatar-state="error"');
    expect(html).not.toContain('<img');
  });

  it('does not render a remote URL directly while the proxy is loading', () => {
    const html = renderToStaticMarkup(
      <Avatar src="https://i0.hdslb.com/bfs/face/a.jpg" className="h-8 w-8" />,
    );
    expect(html).toContain('data-avatar-state="loading"');
    expect(html).not.toContain('i0.hdslb.com');
    expect(html).not.toContain('<img');
  });

  it('renders local and mock resources directly', () => {
    const html = renderToStaticMarkup(<Avatar src="/avatars/avatar-01.svg" alt="用户头像" />);
    expect(html).toContain('src="/avatars/avatar-01.svg"');
    expect(html).toContain('alt="用户头像"');
  });
});
