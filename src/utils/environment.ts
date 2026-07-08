const PRODUCTION_API_HOST = 'api.easicoin.io'

export function resolveApiEnvironmentLabel(baseUrl?: string | null): '正式' | '测试' | '未知' {
  if (!baseUrl) {
    return '未知'
  }

  try {
    const host = new URL(baseUrl).hostname
    return host === PRODUCTION_API_HOST ? '正式' : '测试'
  } catch {
    return baseUrl.includes(PRODUCTION_API_HOST) ? '正式' : '未知'
  }
}
