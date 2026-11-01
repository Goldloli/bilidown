import type { ReactNode } from 'react';

interface FieldRowProps {
  label: string;
  description?: ReactNode;
  children: ReactNode;
  error?: string | null;
  stacked?: boolean;
  htmlFor?: string;
}

/** 偏好设置表单行：标签左、控件右对齐（macOS 系统设置同构） */
export function FieldRow({ label, description, children, error, stacked, htmlFor }: FieldRowProps) {
  if (stacked) {
    return (
      <div className="py-1.5">
        <div className="flex flex-col gap-1.5">
          <label className="text-sm text-fg" htmlFor={htmlFor}>
            {label}
          </label>
          {description && <p className="text-2xs leading-[15px] text-fg2">{description}</p>}
          <div>{children}</div>
          {error && <p className="text-2xs text-danger">{error}</p>}
        </div>
      </div>
    );
  }
  return (
    <div className="flex items-start justify-between gap-6 py-1.5">
      <div className="min-w-0 max-w-[440px]">
        <label className="text-sm text-fg" htmlFor={htmlFor}>
          {label}
        </label>
        {description && <p className="mt-0.5 text-2xs leading-[15px] text-fg2">{description}</p>}
        {error && <p className="mt-0.5 text-2xs text-danger">{error}</p>}
      </div>
      <div className="shrink-0 pt-0.5">{children}</div>
    </div>
  );
}
