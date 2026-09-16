import { storeToRefs } from "pinia";

import { useThemeStore } from "@/stores/theme";

export function useTheme() {
  const store = useThemeStore();
  const { isDark, theme } = storeToRefs(store);

  return { isDark, theme, toggle: store.toggle, initialize: store.initialize };
}
