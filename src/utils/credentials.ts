export interface CredentialDraftValidation {
  mode: 'create' | 'edit'
  apiKey: string
  apiSecret: string
}

export function validateCredentialDraft(draft: CredentialDraftValidation): string | null {
  const hasKey = draft.apiKey.trim().length > 0
  const hasSecret = draft.apiSecret.trim().length > 0
  if (hasKey !== hasSecret) {
    return 'API key and secret must be provided together'
  }
  if (draft.mode === 'create' && !hasKey) {
    return 'API key and secret are required for a new account'
  }
  return null
}
