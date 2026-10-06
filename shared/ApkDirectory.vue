<script setup lang="ts">
import { computed } from "vue";
import { Folder, RefreshCw } from "@lucide/vue";
import { sizeText, type ApkRelease } from "./api";
const props = defineProps<{directory: string; releases: ApkRelease[]; version: string; busy: boolean}>();
const emit = defineEmits<{choose: []; reread: []; select: [string]}>();
const selected = computed(() => props.releases.find(r => r.version === props.version));
</script>
<template>
  <div class="ui-toolbar"><button type="button" :disabled="busy" @click="emit('choose')"><Folder :size="16" />{{ directory ? '更换目录' : '选择 APK 构建目录' }}</button><button v-if="directory" type="button" :disabled="busy" @click="emit('reread')"><RefreshCw :size="16" />重新读取</button></div>
  <p class="ui-muted ui-apk-directory">{{ directory || '目录名称任意；根目录或一级版本子目录中放置 catalog.json 和 APK。' }}</p>
  <div v-if="releases.length" class="ui-field"><label for="apk-directory-version">本地 APK 版本</label><select id="apk-directory-version" :value="version" :disabled="busy" @change="emit('select', ($event.target as HTMLSelectElement).value)"><option v-for="(release, index) in releases" :key="release.version" :value="release.version">{{ release.version }} · {{ release.version_name }}{{ index === 0 ? ' · 最新' : '' }}</option></select></div>
  <template v-if="selected"><p class="ui-muted ui-apk-directory">{{ selected.package_name }} · {{ selected.files[0].name }} · {{ sizeText(selected.files.reduce((sum, file) => sum + file.size, 0)) }}</p><small class="ui-muted">发布编号来自 catalog.versionCode；上传前校验大小和 SHA-256。</small></template>
</template>
