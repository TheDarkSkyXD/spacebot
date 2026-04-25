import colors from "./githubLanguageColors.json";

const COLORS: Record<string, string> = colors;

const DEFAULT_COLOR = "#8b8b8b";

export function languageColor(name: string): string {
	return COLORS[name] ?? DEFAULT_COLOR;
}

export { COLORS as GITHUB_LANGUAGE_COLORS };
