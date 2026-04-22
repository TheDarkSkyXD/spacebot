// LocalStorage-backed user overrides for the mermaid view. Two maps per
// project:
//   - group: { [groupId]: { title?, color? } }
//   - node:  { [fileQname]: color }

export interface GroupOverride {
	title?: string;
	color?: string;
}

export type GroupOverrides = Record<string, GroupOverride>;
export type NodeColorOverrides = Record<string, string>;

const GROUP_KEY = (projectId: string) => `spacebot.codegraph.mermaid.groups.${projectId}`;
const NODE_KEY = (projectId: string) => `spacebot.codegraph.mermaid.nodeColors.${projectId}`;
const COLLAPSED_KEY = (projectId: string) => `spacebot.codegraph.mermaid.collapsed.${projectId}`;

function readJson<T>(key: string, fallback: T): T {
	try {
		const raw = window.localStorage.getItem(key);
		if (!raw) return fallback;
		const parsed = JSON.parse(raw);
		if (parsed && typeof parsed === "object") return parsed as T;
		return fallback;
	} catch {
		return fallback;
	}
}

function writeJson(key: string, value: unknown) {
	try { window.localStorage.setItem(key, JSON.stringify(value)); } catch { /* ignore */ }
}

export function loadGroupOverrides(projectId: string): GroupOverrides {
	return readJson<GroupOverrides>(GROUP_KEY(projectId), {});
}

export function saveGroupOverride(projectId: string, groupId: string, patch: Partial<GroupOverride>) {
	const all = loadGroupOverrides(projectId);
	const current = all[groupId] ?? {};
	const merged: GroupOverride = { ...current, ...patch };
	// Drop empty fields so a reset returns to default.
	if (!merged.title) delete merged.title;
	if (!merged.color) delete merged.color;
	if (Object.keys(merged).length === 0) delete all[groupId];
	else all[groupId] = merged;
	writeJson(GROUP_KEY(projectId), all);
}

export function loadNodeColors(projectId: string): NodeColorOverrides {
	return readJson<NodeColorOverrides>(NODE_KEY(projectId), {});
}

export function saveNodeColor(projectId: string, fileQname: string, color: string | null) {
	const all = loadNodeColors(projectId);
	if (color === null) delete all[fileQname];
	else all[fileQname] = color;
	writeJson(NODE_KEY(projectId), all);
}

// Collapsed-group state. Stored as a flat list of group IDs the user
// has explicitly collapsed or expanded. Missing entries fall back to
// the "default collapsed" policy driven by total file count.
export interface CollapsedState {
	explicit: Record<string, boolean>; // groupId → collapsed?
}

export function loadCollapsed(projectId: string): CollapsedState {
	return readJson<CollapsedState>(COLLAPSED_KEY(projectId), { explicit: {} });
}

export function saveCollapsed(projectId: string, state: CollapsedState) {
	writeJson(COLLAPSED_KEY(projectId), state);
}
