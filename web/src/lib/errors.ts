/** 统一提取 Tauri invoke 错误（AppErrorDTO {message}）与普通异常的可用文案。 */
export function errorMessageOf(error: unknown): string {
  if (error && typeof error === 'object' && 'message' in error) {
    const message = (error as { message: unknown }).message;
    if (typeof message === 'string' && message.trim()) return message;
  }
  if (error instanceof Error) return error.message;
  return String(error);
}
