<script setup lang="ts">
import { Check, Circle, AlertCircle, Ban } from "@lucide/vue";
withDefaults(defineProps<{ tone?: 'success' | 'warning' | 'danger' | 'neutral' }>(), {tone: 'neutral'});
</script>
<template>
  <span class="ui-status-badge" :class="'status-' + tone">
    <Check v-if="tone === 'success'" :size="12" aria-hidden="true" />
    <AlertCircle v-else-if="tone === 'warning'" :size="12" aria-hidden="true" />
    <Ban v-else-if="tone === 'danger'" :size="12" aria-hidden="true" />
    <Circle v-else :size="10" aria-hidden="true" />
    <slot />
  </span>
</template>
