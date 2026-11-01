/** 由 docs/design/app-icon.svg 导出；各尺寸共享母版，避免界面与系统图标分叉。 */
const BRAND_ICON = '/app-icon.svg';

export function BrandMark({ size = 96, title = 'bilidown' }: { size?: number; title?: string }) {
  return (
    <img
      src={BRAND_ICON}
      width={size}
      height={size}
      alt={title}
      draggable={false}
      style={{ display: 'block', flexShrink: 0 }}
    />
  );
}

/** 标题栏使用同一矢量，在 16px 以上保持主体与下载芯片的位置一致。 */
export function BrandGlyph({ size = 18 }: { size?: number }) {
  return (
    <img
      src={BRAND_ICON}
      width={size}
      height={size}
      alt=""
      aria-hidden="true"
      draggable={false}
      style={{ display: 'block', flexShrink: 0 }}
    />
  );
}
