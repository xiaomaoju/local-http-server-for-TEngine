import { ref } from "vue";
export const theme = ref<"dark" | "light">(
  localStorage.getItem("tengine_theme") === "light" ? "light" : "dark",
);
export function applyTheme() {
  document.documentElement.dataset.theme = theme.value;
  document.documentElement.style.colorScheme = theme.value;
}
export function toggleTheme() {
  theme.value = theme.value === "dark" ? "light" : "dark";
  localStorage.setItem("tengine_theme", theme.value);
  applyTheme();
}
applyTheme();
