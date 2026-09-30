<script setup lang="ts">
import { useId } from "vue";
defineProps<{ modelValue: string; label: string; options: { value: string; label: string }[]; disabled?: boolean; hideLabel?: boolean }>();
const emit = defineEmits<{ 'update:modelValue': [value: string] }>();
const id = useId();
</script>
<template>
  <div class="ui-choice-field">
    <span :id="id + '-label'" :class="hideLabel ? 'ui-sr-only' : 'ui-choice-label'">{{ label }}</span>
    <div class="ui-segmented" role="radiogroup" :aria-labelledby="id + '-label'">
      <label v-for="option in options" :key="option.value" class="ui-segment" :class="{ selected: modelValue === option.value }">
        <input type="radio" :name="id" :value="option.value" :checked="modelValue === option.value" :disabled="disabled" @change="emit('update:modelValue', option.value)" />
        <span>{{ option.label }}</span>
      </label>
    </div>
  </div>
</template>
