import { readFile } from 'node:fs/promises'
import { resolve } from 'node:path'
import { describe, expect, it } from 'vitest'

const workflowPath = (name: string) => resolve(process.cwd(), '.github', 'workflows', name)

const readWorkflow = (name: string) => readFile(workflowPath(name), 'utf8')

describe('news release workflow contract', () => {
  it('requires named fixed-source and token secrets plus the source epoch input', async () => {
    const workflow = await readWorkflow('tauri-build-reusable.yml')

    expect(workflow).toMatch(
      /secrets:\s*\r?\n\s+news_api_base_url:\s*\r?\n\s+required:\s*true\s*\r?\n\s+news_api_token:\s*\r?\n\s+required:\s*true/,
    )
    expect(workflow).toMatch(
      /news_source_epoch:\s*\r?\n\s+required:\s*true\s*\r?\n\s+type:\s*string/,
    )
    const inputsBlock = workflow.slice(workflow.indexOf('inputs:'), workflow.indexOf('secrets:'))
    expect(inputsBlock).not.toContain('news_api_base_url:')
  })

  it.each(['release.yml', 'release-please.yml'])(
    '%s explicitly maps repository secrets and the source epoch variable',
    async (name) => {
      const workflow = await readWorkflow(name)

      expect(workflow).toContain(
        'news_source_epoch: ${{ vars.EASIFLUX_NEWS_SOURCE_EPOCH }}',
      )
      expect(workflow).toContain(
        'news_api_base_url: ${{ secrets.EASIFLUX_NEWS_API_BASE_URL }}',
      )
      expect(workflow).toContain(
        'news_api_token: ${{ secrets.EASIFLUX_NEWS_API_TOKEN }}',
      )
      expect(workflow).not.toContain('secrets: inherit')
      expect(workflow).not.toContain('vars.EASIFLUX_NEWS_API_BASE_URL')
    },
  )

  it('scopes source and token values to the two compilation steps', async () => {
    const workflow = await readWorkflow('tauri-build-reusable.yml')
    const baseUrlEnvLines = workflow.match(/^\s+EASIFLUX_NEWS_API_BASE_URL:/gm) ?? []
    const epochEnvLines = workflow.match(/^\s+EASIFLUX_NEWS_SOURCE_EPOCH:/gm) ?? []
    const tokenEnvLines = workflow.match(/^\s+EASIFLUX_NEWS_API_TOKEN:/gm) ?? []

    expect(baseUrlEnvLines).toHaveLength(2)
    expect(epochEnvLines).toHaveLength(2)
    expect(tokenEnvLines).toHaveLength(2)
    expect(workflow).toMatch(
      /name:\s*Build news provisioning utility[\s\S]*?env:[\s\S]*?EASIFLUX_NEWS_API_BASE_URL:\s*\$\{\{ secrets\.news_api_base_url \}\}[\s\S]*?EASIFLUX_NEWS_API_TOKEN:\s*\$\{\{ secrets\.news_api_token \}\}[\s\S]*?EASIFLUX_NEWS_SOURCE_EPOCH:\s*\$\{\{ inputs\.news_source_epoch \}\}[\s\S]*?cargo build --release --manifest-path src-tauri\/Cargo\.toml --bin easiflux-news-provision/,
    )
    expect(workflow).toMatch(
      /name:\s*Build and publish desktop release[\s\S]*?env:[\s\S]*?EASIFLUX_NEWS_API_BASE_URL:\s*\$\{\{ secrets\.news_api_base_url \}\}[\s\S]*?EASIFLUX_NEWS_API_TOKEN:\s*\$\{\{ secrets\.news_api_token \}\}[\s\S]*?EASIFLUX_NEWS_SOURCE_EPOCH:\s*\$\{\{ inputs\.news_source_epoch \}\}/,
    )
  })

  it('never interpolates or echoes source and token secrets in a run script', async () => {
    const workflow = await readWorkflow('tauri-build-reusable.yml')
    const runBlocks = [...workflow.matchAll(/^\s+run:\s*(?:\|\s*)?\r?\n((?:\s{8,}.*\r?\n?)*)/gm)]
      .map((match) => match[1])
      .join('\n')

    expect(runBlocks).not.toContain('${{ secrets.news_api_base_url }}')
    expect(runBlocks).not.toContain('${{ secrets.news_api_token }}')
    expect(runBlocks).not.toContain('${{ secrets.EASIFLUX_NEWS_API_BASE_URL }}')
    expect(runBlocks).not.toContain('${{ secrets.EASIFLUX_NEWS_API_TOKEN }}')
    expect(runBlocks).not.toMatch(/echo|write-(?:host|output)|print/i)
  })

  it('pins the secret-bearing Tauri action to the audited v0.6.2 commit', async () => {
    const workflow = await readWorkflow('tauri-build-reusable.yml')

    expect(workflow).toContain(
      'uses: tauri-apps/tauri-action@84b9d35b5fc46c1e45415bdb6144030364f7ebc5 # v0.6.2',
    )
    expect(workflow).not.toMatch(/uses:\s*tauri-apps\/tauri-action@(v\d+|main|dev)\b/)
  })

  it('installs the persistent Linux Keyring dependency', async () => {
    const workflow = await readWorkflow('tauri-build-reusable.yml')

    expect(workflow).toContain('libdbus-1-dev')
  })

  it('publishes host-triple provisioning binaries and checksums without overwriting', async () => {
    const workflow = await readWorkflow('tauri-build-reusable.yml')

    expect(workflow).toContain('rustc -vV')
    expect(workflow).toMatch(/\^host: /)
    expect(workflow).toContain('easiflux-news-provision-$hostTriple$extension')
    expect(workflow).toContain('Get-FileHash -Algorithm SHA256')
    expect(workflow).toContain('shasum -a 256')
    expect(workflow).toContain('sha256sum')
    expect(workflow).toContain('gh release upload')
    expect(workflow).toContain('.sha256')
    expect(workflow).not.toContain('--clobber')
  })

  it('uploads provisioning assets only after the desktop action has created the release', async () => {
    const workflow = await readWorkflow('tauri-build-reusable.yml')

    expect(workflow.indexOf('- name: Upload news provisioning release assets')).toBeGreaterThan(
      workflow.indexOf('- name: Build and publish desktop release'),
    )
  })

  it('passes release upload values through step environment instead of script interpolation', async () => {
    const workflow = await readWorkflow('tauri-build-reusable.yml')
    const uploadStep = workflow.slice(
      workflow.indexOf('- name: Upload news provisioning release assets'),
    )
    const runBlock = uploadStep.slice(uploadStep.indexOf('run:'))

    expect(uploadStep).toContain('NEWS_RELEASE_TAG: ${{ inputs.tag_name }}')
    expect(uploadStep).toContain(
      'NEWS_PROVISION_ASSET: ${{ steps.news-provision.outputs.asset }}',
    )
    expect(uploadStep).toContain(
      'NEWS_PROVISION_CHECKSUM: ${{ steps.news-provision.outputs.checksum }}',
    )
    expect(runBlock).not.toContain('${{ inputs.tag_name }}')
    expect(runBlock).toContain(
      'gh release upload -- $env:NEWS_RELEASE_TAG $env:NEWS_PROVISION_ASSET $env:NEWS_PROVISION_CHECKSUM',
    )
    expect(runBlock).not.toMatch(/gh release upload \$env:NEWS_RELEASE_TAG/)
  })
})
