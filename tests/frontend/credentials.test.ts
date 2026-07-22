import { describe, expect, it } from 'vitest'
import { validateCredentialDraft } from '../../src/utils/credentials'

describe('validateCredentialDraft', () => {
  it('requires a complete pair for create and permits blank edit credentials', () => {
    expect(validateCredentialDraft({ mode: 'create', apiKey: '', apiSecret: '' })).toBeTruthy()
    expect(validateCredentialDraft({ mode: 'edit', apiKey: '', apiSecret: '' })).toBeNull()
    expect(validateCredentialDraft({ mode: 'edit', apiKey: 'new', apiSecret: '' })).toBeTruthy()
    expect(
      validateCredentialDraft({ mode: 'edit', apiKey: 'new', apiSecret: 'secret' }),
    ).toBeNull()
  })
})
