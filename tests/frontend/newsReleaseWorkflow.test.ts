import { readFile } from 'node:fs/promises'
import { resolve } from 'node:path'
import { describe, expect, it } from 'vitest'

const workflowPath = (name: string) => resolve(process.cwd(), '.github', 'workflows', name)

const readWorkflow = (name: string) => readFile(workflowPath(name), 'utf8')

describe('news release workflow contract', () => {
  it('requires the two non-secret fixed-source inputs in the reusable workflow', async () => {
    const workflow = await readWorkflow('tauri-build-reusable.yml')

    expect(workflow).toMatch(
      /news_api_base_url:\s*\r?\n\s+required:\s*true\s*\r?\n\s+type:\s*string/,
    )
    expect(workflow).toMatch(
      /news_source_epoch:\s*\r?\n\s+required:\s*true\s*\r?\n\s+type:\s*string/,
    )
  })

  it.each(['release.yml', 'release-please.yml'])(
    '%s passes the protected repository variables exactly',
    async (name) => {
      const workflow = await readWorkflow(name)

      expect(workflow).toContain(
        'news_api_base_url: ${{ vars.EASIFLUX_NEWS_API_BASE_URL }}',
      )
      expect(workflow).toContain(
        'news_source_epoch: ${{ vars.EASIFLUX_NEWS_SOURCE_EPOCH }}',
      )
    },
  )

  it('scopes fixed-source values to the two compilation steps', async () => {
    const workflow = await readWorkflow('tauri-build-reusable.yml')
    const baseUrlEnvLines = workflow.match(/^\s+EASIFLUX_NEWS_API_BASE_URL:/gm) ?? []
    const epochEnvLines = workflow.match(/^\s+EASIFLUX_NEWS_SOURCE_EPOCH:/gm) ?? []

    expect(baseUrlEnvLines).toHaveLength(2)
    expect(epochEnvLines).toHaveLength(2)
    expect(workflow).toMatch(
      /name:\s*Build news provisioning utility[\s\S]*?env:[\s\S]*?EASIFLUX_NEWS_API_BASE_URL:\s*\$\{\{ inputs\.news_api_base_url \}\}[\s\S]*?EASIFLUX_NEWS_SOURCE_EPOCH:\s*\$\{\{ inputs\.news_source_epoch \}\}[\s\S]*?cargo build --release --manifest-path src-tauri\/Cargo\.toml --bin easiflux-news-provision/,
    )
    expect(workflow).toMatch(
      /name:\s*Build and publish desktop release[\s\S]*?env:[\s\S]*?EASIFLUX_NEWS_API_BASE_URL:\s*\$\{\{ inputs\.news_api_base_url \}\}[\s\S]*?EASIFLUX_NEWS_SOURCE_EPOCH:\s*\$\{\{ inputs\.news_source_epoch \}\}/,
    )
  })

  it('does not expose a news bearer token through release workflow data', async () => {
    const workflows = await Promise.all(
      ['tauri-build-reusable.yml', 'release.yml', 'release-please.yml'].map(readWorkflow),
    )
    const releaseContract = workflows.join('\n')

    expect(releaseContract).not.toMatch(
      /EASIFLUX_NEWS_API_TOKEN|NEWS_API_TOKEN|NEWS_TOKEN|news_api_token|bearer\s+token/i,
    )
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
