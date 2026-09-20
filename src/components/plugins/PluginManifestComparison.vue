<script setup lang="ts">
import { computed } from 'vue'
import { pluginPageLabel } from '../../services/pluginNavigation'
import {
  comparePluginManifests,
  pluginComputeCodeChanged,
  pluginComputeModuleByteLength,
} from '../../services/pluginManifestDiff'
import type {
  PluginCatalogItem,
  PluginCommandContribution,
  PluginManifest,
  PluginManifestField,
} from '../../types/plugin'
import {
  pluginManagementLabel,
  pluginSourceLabel,
  pluginStatusLabel,
} from './pluginPresentation'

const props = defineProps<{
  current: PluginCatalogItem
  incoming: PluginManifest
  versionRelation: 'incomingLower' | 'samePrecedence' | 'incomingHigher'
}>()

const diff = computed(() => comparePluginManifests(props.current.manifest, props.incoming))

const fieldLabels: Record<PluginManifestField, string> = {
  schemaVersion: '清单架构',
  publisherId: '发布者 ID',
  publisher: '发布者',
  name: '名称',
  description: '描述',
  version: '版本',
  requestedCapabilities: '请求权限',
}

const versionRelationCopy = {
  incomingLower: '候选版本优先级较低。',
  samePrecedence: '候选版本与当前版本的版本优先级相同；构建元数据不参与优先级比较。',
  incomingHigher: '候选版本优先级较高。',
} as const

function fieldValue(manifest: PluginManifest, field: PluginManifestField): string {
  if (field === 'requestedCapabilities') {
    return manifest.requestedCapabilities.length > 0
      ? manifest.requestedCapabilities.join('、')
      : '无'
  }
  return String(manifest[field])
}

function commandLabel(command: PluginCommandContribution): string {
  if (command.actionId === 'host.showInfo') return `显示信息：${command.title}`
  if (command.actionId === 'host.openPage') {
    return `打开页面：${pluginPageLabel(command.params.destination)}`
  }
  return `运行本地计算：${command.title}`
}

function commandDetails(
  command: PluginCommandContribution,
): { label: string; value: string }[] {
  const common = [
    { label: '贡献 ID', value: command.contributionId },
    { label: '标题', value: command.title },
    { label: '宿主动作', value: commandLabel(command) },
  ]
  if (command.actionId === 'host.showInfo') {
    return [
      ...common,
      { label: '信息标题', value: command.params.title },
      { label: '信息正文', value: command.params.text },
    ]
  }
  if (command.actionId === 'sandbox.computeSeries') {
    return [
      ...common,
      { label: '运行时', value: command.params.runtime },
      { label: 'ABI', value: command.params.abi },
      { label: '代码模块', value: `${pluginComputeModuleByteLength(command)} 字节` },
      { label: '参数名称', value: command.params.parameter.label },
      { label: '参数默认值', value: String(command.params.parameter.default) },
      {
        label: '参数范围',
        value: `${command.params.parameter.min} 至 ${command.params.parameter.max}`,
      },
    ]
  }
  return [
    ...common,
    { label: '宿主目标', value: pluginPageLabel(command.params.destination) },
  ]
}
</script>

<template>
  <section class="plugin-manifest-comparison" aria-label="现有插件与候选清单比较">
    <dl class="plugin-manifest-comparison__context">
      <div><dt>当前来源</dt><dd><bdi>{{ pluginSourceLabel(props.current.source) }}</bdi></dd></div>
      <div><dt>当前管理方式</dt><dd><bdi>{{ pluginManagementLabel(props.current.management) }}</bdi></dd></div>
      <div><dt>当前状态</dt><dd><bdi>{{ pluginStatusLabel(props.current.status) }}</bdi></dd></div>
    </dl>
    <section class="plugin-manifest-comparison__identity">
      <h3>插件身份</h3>
      <dl>
        <div>
          <dt>插件 ID</dt>
          <dd data-testid="plugin-import-shared-id">
            <bdi>{{ props.incoming.id }}</bdi>
          </dd>
        </div>
      </dl>
      <div class="plugin-manifest-comparison__identity-pair">
        <section
          class="plugin-manifest-comparison__identity-card"
          data-testid="plugin-import-current-identity"
        >
          <h4>当前清单</h4>
          <dl>
            <div><dt>名称</dt><dd><bdi>{{ props.current.manifest.name }}</bdi></dd></div>
            <div><dt>版本</dt><dd><bdi>{{ props.current.manifest.version }}</bdi></dd></div>
            <div><dt>发布者</dt><dd><bdi>{{ props.current.manifest.publisher }}</bdi></dd></div>
            <div><dt>发布者 ID</dt><dd><bdi>{{ props.current.manifest.publisherId }}</bdi></dd></div>
          </dl>
        </section>
        <section
          class="plugin-manifest-comparison__identity-card"
          data-testid="plugin-import-candidate-identity"
        >
          <h4>候选清单</h4>
          <dl>
            <div><dt>名称</dt><dd><bdi>{{ props.incoming.name }}</bdi></dd></div>
            <div><dt>版本</dt><dd><bdi>{{ props.incoming.version }}</bdi></dd></div>
            <div><dt>发布者</dt><dd><bdi>{{ props.incoming.publisher }}</bdi></dd></div>
            <div><dt>发布者 ID</dt><dd><bdi>{{ props.incoming.publisherId }}</bdi></dd></div>
          </dl>
        </section>
      </div>
      <p
        class="plugin-manifest-comparison__publisher-notice"
        data-testid="plugin-import-publisher-notice"
      >
        当前与候选清单中的发布者名称和发布者 ID 均由清单作者填写，未经认证。
      </p>
    </section>
    <p data-testid="plugin-import-version-relation">
      {{ versionRelationCopy[props.versionRelation] }}
    </p>

    <p
      v-if="diff.sameContent"
      class="plugin-manifest-comparison__unchanged"
      data-testid="plugin-manifest-unchanged"
    >
      清单内容未变化；这里只显示比较结果，不会执行导入。
    </p>

    <section v-if="diff.changedFields.length > 0" class="plugin-manifest-comparison__section">
      <h3>元数据变更</h3>
      <dl class="plugin-manifest-comparison__fields">
        <div v-for="field in diff.changedFields" :key="field">
          <dt>{{ fieldLabels[field] }}</dt>
          <dd>
            <span>当前</span><bdi>{{ fieldValue(props.current.manifest, field) }}</bdi>
            <span>候选</span><bdi>{{ fieldValue(props.incoming, field) }}</bdi>
          </dd>
        </div>
      </dl>
    </section>

    <p v-if="diff.publisherIdChanged" class="plugin-manifest-comparison__warning">
      发布者 ID 已变化；此字段仅来自清单，不代表已认证身份。
    </p>
    <p v-if="diff.orderChanged" class="plugin-manifest-comparison__order">
      公共命令的相对顺序已变化。
    </p>

    <section v-if="diff.added.length > 0" class="plugin-manifest-comparison__section">
      <h3>新增命令</h3>
      <details v-for="command in diff.added" :key="command.contributionId">
        <summary><bdi>{{ command.contributionId }} · {{ commandLabel(command) }}</bdi></summary>
        <dl>
          <div v-for="detail in commandDetails(command)" :key="detail.label">
            <dt>{{ detail.label }}</dt><dd><bdi>{{ detail.value }}</bdi></dd>
          </div>
        </dl>
      </details>
    </section>

    <section v-if="diff.removed.length > 0" class="plugin-manifest-comparison__section">
      <h3>移除命令</h3>
      <details v-for="command in diff.removed" :key="command.contributionId">
        <summary><bdi>{{ command.contributionId }} · {{ commandLabel(command) }}</bdi></summary>
        <dl>
          <div v-for="detail in commandDetails(command)" :key="detail.label">
            <dt>{{ detail.label }}</dt><dd><bdi>{{ detail.value }}</bdi></dd>
          </div>
        </dl>
      </details>
    </section>

    <section v-if="diff.changed.length > 0" class="plugin-manifest-comparison__section">
      <h3>变更命令</h3>
      <details v-for="change in diff.changed" :key="change.after.contributionId">
        <summary><bdi>{{ change.after.contributionId }} · {{ commandLabel(change.after) }}</bdi></summary>
        <p
          v-if="pluginComputeCodeChanged(change.before, change.after)"
          class="plugin-manifest-comparison__warning"
          data-testid="plugin-compute-code-change"
        >
          可执行代码内容已变化；比较仅显示模块大小，不显示 Base64 内容。
        </p>
        <div class="plugin-manifest-comparison__command-change">
          <section>
            <h4>当前</h4>
            <dl>
              <div v-for="detail in commandDetails(change.before)" :key="detail.label">
                <dt>{{ detail.label }}</dt><dd><bdi>{{ detail.value }}</bdi></dd>
              </div>
            </dl>
          </section>
          <section>
            <h4>候选</h4>
            <dl>
              <div v-for="detail in commandDetails(change.after)" :key="detail.label">
                <dt>{{ detail.label }}</dt><dd><bdi>{{ detail.value }}</bdi></dd>
              </div>
            </dl>
          </section>
        </div>
      </details>
    </section>
  </section>
</template>
