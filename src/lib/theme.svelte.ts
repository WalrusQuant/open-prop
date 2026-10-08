const KEY = "open-trend-theme";

export const theme = $state({ mode: "light" as "light" | "dark" });

export function initTheme() {
  const stored = localStorage.getItem(KEY);
  if (stored === "light" || stored === "dark") theme.mode = stored;
  else theme.mode = window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
  document.documentElement.dataset.theme = theme.mode;
}

export function toggleTheme() {
  theme.mode = theme.mode === "light" ? "dark" : "light";
  localStorage.setItem(KEY, theme.mode);
  document.documentElement.dataset.theme = theme.mode;
}
