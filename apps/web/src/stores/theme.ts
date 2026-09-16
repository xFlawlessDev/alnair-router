import { defineStore } from "pinia";
import { computed, shallowRef } from "vue";

export type Theme = "light" | "dark";

function getInitialTheme(): Theme {
  if (typeof window === "undefined") return "light";

  const stored = window.localStorage.getItem("theme");
  if (stored === "light" || stored === "dark") return stored;

  return typeof window.matchMedia === "function" &&
    window.matchMedia("(prefers-color-scheme: dark)").matches
    ? "dark"
    : "light";
}

export const useThemeStore = defineStore("theme", () => {
  const theme = shallowRef<Theme>(getInitialTheme());
  const isDark = computed(() => theme.value === "dark");

  function apply(value: Theme): void {
    theme.value = value;
    document.documentElement.classList.toggle("dark", value === "dark");
    window.localStorage.setItem("theme", value);
  }

  function toggle(): void {
    apply(isDark.value ? "light" : "dark");
  }

  function initialize(): void {
    document.documentElement.classList.toggle("dark", isDark.value);
  }

  return { theme, isDark, apply, toggle, initialize };
});
