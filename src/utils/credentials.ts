export interface CredentialDraftValidation {
  mode: 'create' | 'edit'
  apiKey: string
  apiSecret: string
}

export function validateCredentialDraft(draft: CredentialDraftValidation): string | null {
  const hasKey = draft.apiKey.trim().length > 0
  const hasSecret = draft.apiSecret.trim().length > 0
  if (hasKey !== hasSecret) {
    return 'API 访问密钥和签名密钥必须同时填写'
  }
  if (draft.mode === 'create' && !hasKey) {
    return '新账户必须填写 API 访问密钥和签名密钥'
  }
  return null
}
