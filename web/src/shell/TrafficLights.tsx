/** mock/浏览器评审模式下的伪交通灯（Tauri 下由系统绘制，本组件不渲染） */
export function TrafficLights() {
  return (
    <div className="pointer-events-none absolute left-4 top-1/2 flex -translate-y-1/2 items-center gap-[9px]" aria-hidden="true">
      {['#ff5f57', '#febc2e', '#28c840'].map((color) => (
        <span
          key={color}
          className="block h-[13px] w-[13px] rounded-full"
          style={{
            background: color,
            boxShadow: 'inset 0 0 0 0.5px rgba(0,0,0,0.12)',
          }}
        />
      ))}
    </div>
  );
}
