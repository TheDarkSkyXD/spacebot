// React Flow custom node used as a parent container for a relationship
// group. Renders a rounded box whose border/header is tinted by the
// group's color override. Title is inline-editable via the pencil icon;
// color is picked via the shared NodeColorPicker popover.

import { memo, useEffect, useRef, useState } from "react";
import { Handle, Position, type NodeProps } from "@xyflow/react";
import * as Popover from "@radix-ui/react-popover";
import { NodeColorPicker } from "./NodeColorPicker";

export const DEFAULT_GROUP_COLOR = "#4a7c9b";

export interface GroupContainerData extends Record<string, unknown> {
	groupId: string;
	title: string;
	color: string;
	fileCount: number;
	collapsed: boolean;
	onRenameGroup: (groupId: string, title: string | null) => void;
	onRecolorGroup: (groupId: string, color: string | null) => void;
	onToggleCollapsed: (groupId: string) => void;
}

export const GroupContainer = memo(({ data }: NodeProps) => {
	const { groupId, title, color, fileCount, collapsed, onRenameGroup, onRecolorGroup, onToggleCollapsed } = data as GroupContainerData;
	const [editing, setEditing] = useState(false);
	const [draftTitle, setDraftTitle] = useState(title);
	const inputRef = useRef<HTMLInputElement>(null);

	useEffect(() => {
		if (!editing) setDraftTitle(title);
	}, [title, editing]);

	useEffect(() => {
		if (editing) {
			inputRef.current?.focus();
			inputRef.current?.select();
		}
	}, [editing]);

	const commit = () => {
		const trimmed = draftTitle.trim();
		if (trimmed === "" || trimmed === title) {
			setEditing(false);
			setDraftTitle(title);
			return;
		}
		onRenameGroup(groupId, trimmed);
		setEditing(false);
	};

	const cancel = () => {
		setDraftTitle(title);
		setEditing(false);
	};

	return (
		<div
			className="relative h-full w-full rounded-xl border-2"
			style={{ borderColor: `${color}aa`, background: `${color}0d` }}
		>
			{/* Handles for inter-group (bundled) edges. Invisible — they
			    live at top-middle / bottom-middle of the container so
			    bundled bezier edges read as group-to-group arcs. */}
			<Handle type="target" position={Position.Top} className="!h-1 !w-1 !border-0 !bg-transparent opacity-0" />
			<Handle type="source" position={Position.Bottom} className="!h-1 !w-1 !border-0 !bg-transparent opacity-0" />
			{/* Title bar — opaque so the dotted edges never visually cross
			    through it. Accent line on the bottom instead of a full
			    translucent fill. */}
			<div
				className="nodrag flex items-center gap-2 rounded-t-[10px] bg-app-darkBox px-3 py-2"
				style={{ borderBottom: `2px solid ${color}` }}
			>
				<Popover.Root>
					<Popover.Trigger asChild>
						<button
							className="h-3 w-3 shrink-0 rounded-full border border-white/30 transition-transform hover:scale-110"
							style={{ background: color }}
							title="Change group color"
							onClick={(e) => e.stopPropagation()}
						/>
					</Popover.Trigger>
					<Popover.Portal>
						<Popover.Content
							side="bottom"
							sideOffset={6}
							className="z-50 rounded-lg border border-app-line bg-app-darkBox p-2 shadow-xl"
							onClick={(e) => e.stopPropagation()}
						>
							<NodeColorPicker
								currentColor={color}
								defaultColor={DEFAULT_GROUP_COLOR}
								onSelect={(c) => onRecolorGroup(groupId, c)}
								onReset={() => onRecolorGroup(groupId, null)}
							/>
						</Popover.Content>
					</Popover.Portal>
				</Popover.Root>

				{editing ? (
					<input
						ref={inputRef}
						value={draftTitle}
						onChange={(e) => setDraftTitle(e.target.value)}
						onKeyDown={(e) => {
							if (e.key === "Enter") commit();
							else if (e.key === "Escape") cancel();
						}}
						onBlur={commit}
						className="min-w-0 flex-1 rounded border border-app-line bg-app px-1.5 py-0.5 text-xs text-ink outline-none focus:border-accent"
						onClick={(e) => e.stopPropagation()}
					/>
				) : (
					<span
						className="min-w-0 flex-1 truncate text-xs font-semibold text-ink"
						title={title}
					>
						{title}
					</span>
				)}

				<span className="shrink-0 font-mono text-[10px] text-ink-faint">
					{fileCount}
				</span>

				<button
					onClick={(e) => {
						e.stopPropagation();
						setEditing((v) => !v);
					}}
					className="shrink-0 rounded p-0.5 text-ink-faint transition-colors hover:bg-app-hover hover:text-ink"
					title="Edit group title"
				>
					<svg viewBox="0 0 16 16" fill="currentColor" className="h-3 w-3">
						<path d="M13.4 1.6a2.1 2.1 0 0 0-3 0L3.3 8.7a1 1 0 0 0-.2.4l-1 3.5a.5.5 0 0 0 .6.6l3.5-1a1 1 0 0 0 .4-.2l7.1-7.1a2.1 2.1 0 0 0 0-3ZM11 3.2l1.8 1.8-5.7 5.7-2.3.6.6-2.3Z" />
					</svg>
				</button>

				<button
					onClick={(e) => {
						e.stopPropagation();
						onToggleCollapsed(groupId);
					}}
					className="shrink-0 rounded p-0.5 text-ink-faint transition-colors hover:bg-app-hover hover:text-ink"
					title={collapsed ? "Expand group" : "Collapse group"}
				>
					<svg
						viewBox="0 0 16 16"
						fill="none"
						stroke="currentColor"
						strokeWidth="2"
						strokeLinecap="round"
						strokeLinejoin="round"
						className={"h-3 w-3 transition-transform " + (collapsed ? "-rotate-90" : "")}
					>
						<polyline points="4 6 8 10 12 6" />
					</svg>
				</button>
			</div>
			{collapsed && (
				<button
					type="button"
					onClick={(e) => {
						e.stopPropagation();
						onToggleCollapsed(groupId);
					}}
					className="nodrag absolute inset-x-3 bottom-2 text-left text-[10px] text-ink-faint transition-colors hover:text-ink"
				>
					{fileCount} {fileCount === 1 ? "file" : "files"} hidden · click to expand
				</button>
			)}
		</div>
	);
});

GroupContainer.displayName = "GroupContainer";
