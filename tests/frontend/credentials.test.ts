import { describe, expect, it } from 'vitest'
import { validateCredentialDraft } from '../../src/utils/credentials'

describe('validateCredentialDraft', () => {
  it('requires a complete pair for create and permits blank edit credentials', () => {
    expect(validateCredentialDraft({ mode: 'create', apiKey: '', apiSecret: '' }))
      .toBe('新账户必须填写 API 访问密钥和签名密钥')
    expect(validateCredentialDraft({ mode: 'edit', apiKey: '', apiSecret: '' })).toBeNull()
    expect(validateCredentialDraft({ mode: 'edit', apiKey: 'new', apiSecret: '' }))
      .toBe('API 访问密钥和签名密钥必须同时填写')
    expect(
      validateCredentialDraft({ mode: 'edit', apiKey: 'new', apiSecret: 'secret' }),
    ).toBeNull()
  })
})
