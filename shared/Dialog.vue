<script setup lang="ts">
import { ref, watch, nextTick, onMounted, onBeforeUnmount, useId } from "vue";
import { X } from "@lucide/vue";
const props = withDefaults(
  defineProps<{
    open: boolean;
    title: string;
    drawer?: boolean;
    wide?: boolean;
    busy?: boolean;
    dismissible?: boolean;
  }>(),
  { dismissible: true },
);
const emit = defineEmits<{ close: [] }>();
const element = ref<HTMLDialogElement>();
const headingId = useId();
let previous: HTMLElement | null = null;
let pointerTrigger: HTMLElement | null = null;
function rememberTrigger(event: PointerEvent) {
  if (!props.open && event.target instanceof Element)
    pointerTrigger = event.target.closest<HTMLElement>(
      "button, a[href], input, select, textarea, [tabindex]",
    );
}
function keyboardTrigger() {
  if (!props.open) pointerTrigger = null;
}
onMounted(() => {
  document.addEventListener("pointerdown", rememberTrigger, true);
  document.addEventListener("keydown", keyboardTrigger, true);
});
function trapFocus(event: KeyboardEvent) {
  if (event.key !== "Tab" || !element.value) return;
  const targets = Array.from(
    element.value.querySelectorAll<HTMLElement>(
      "button, a[href], input, select, textarea, summary, [tabindex]",
    ),
  ).filter(
    (target) =>
      target.tabIndex >= 0 &&
      !target.matches(":disabled") &&
      target.getClientRects().length > 0,
  );
  const first = targets[0];
  const last = targets[targets.length - 1];
  if (!first) {
    event.preventDefault();
    element.value.focus();
  } else if (
    event.shiftKey &&
    (document.activeElement === first ||
      document.activeElement === element.value)
  ) {
    event.preventDefault();
    last.focus();
  } else if (!event.shiftKey && document.activeElement === last) {
    event.preventDefault();
    first.focus();
  }
}
watch(
  () => props.open,
  async (open) => {
    await nextTick();
    if (open && !element.value?.open) {
      // WebKit does not focus buttons on pointer clicks; remember the actual opener.
      previous = pointerTrigger?.isConnected
        ? pointerTrigger
        : (document.activeElement as HTMLElement);
      pointerTrigger = null;
      element.value?.showModal();
    }
    if (!open && element.value?.open) {
      element.value.close();
      previous?.focus();
    }
  },
  { immediate: true },
);
onBeforeUnmount(() => {
  document.removeEventListener("pointerdown", rememberTrigger, true);
  document.removeEventListener("keydown", keyboardTrigger, true);
  element.value?.close();
});
</script>
<template>
  <dialog
    ref="element"
    class="ui-dialog"
    :class="{ 'ui-drawer': drawer, 'ui-dialog-wide': wide }"
    :aria-labelledby="headingId"
    closedby="closerequest"
    @keydown="trapFocus"
    @cancel.prevent="!busy && dismissible !== false && emit('close')"
  >
    <header>
      <h2 :id="headingId">{{ title }}</h2>
      <button
        class="ui-icon"
        type="button"
        aria-label="关闭"
        :disabled="busy || dismissible === false"
        @click="emit('close')"
      >
        <X :size="16" />
      </button>
    </header>
    <fieldset class="ui-dialog-body" :disabled="busy"><slot /></fieldset>
    <footer v-if="$slots.footer"><slot name="footer" /></footer>
  </dialog>
</template>
