<script setup lang="ts">
import { useId, ref, onBeforeUnmount, watch } from "vue";
const props = defineProps<{ label: string; accessibleLabel?: string; disabled?: boolean; reason?: string; danger?: boolean }>();
const emit = defineEmits<{ click: [event: MouseEvent] }>();
const id = useId();
const position = ref<{left: string; top: string; transform: string} | null>(null);
function hide() {
  position.value = null;
  window.removeEventListener('scroll', hide, true);
  window.removeEventListener('resize', hide);
}
function show(event: Event) {
  if (!props.disabled || !props.reason) return;
  const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
  const above = rect.top > 80;
  position.value = {left: Math.max(8, Math.min(rect.left, innerWidth - 268)) + 'px', top: (above ? rect.top - 8 : rect.bottom + 8) + 'px', transform: above ? 'translateY(-100%)' : 'none'};
  window.addEventListener('scroll', hide, true);
  window.addEventListener('resize', hide);
}
watch(() => props.disabled, hide);
onBeforeUnmount(hide);
</script>
<template>
  <span class="ui-action-wrap" :tabindex="disabled && reason ? 0 : undefined" :role="disabled && reason ? 'group' : undefined" :aria-label="disabled && reason ? (accessibleLabel || label) : undefined" :aria-describedby="disabled && reason ? id : undefined" @pointerenter="show" @pointerleave="hide" @focusin="show" @focusout="hide" @keydown.esc="hide">
    <button type="button" class="ui-action" :class="{danger}" :disabled="disabled" :aria-label="accessibleLabel || label" @click="emit('click', $event)"><slot name="icon" />{{ label }}</button>
    <span v-if="disabled && reason" :id="id" class="ui-sr-only">{{ reason }}</span>
  </span>
  <Teleport to="body"><span v-if="position" class="ui-action-help" :style="position" aria-hidden="true">{{ reason }}</span></Teleport>
</template>
