<script setup lang="ts">
defineProps<{ modelValue: boolean; label: string; disabled?: boolean }>();
const emit = defineEmits<{ 'update:modelValue': [value: boolean] }>();
</script>
<template>
  <button class="ui-switch" type="button" role="switch" :aria-label="label" :aria-checked="modelValue" :disabled="disabled" @click="emit('update:modelValue', !modelValue)">
    <span class="ui-switch-label">{{ label }}</span>
    <span class="ui-toggle-track" aria-hidden="true"><span /></span>
    <span class="ui-switch-value" aria-hidden="true">{{ modelValue ? '开启' : '关闭' }}</span>
  </button>
</template>
