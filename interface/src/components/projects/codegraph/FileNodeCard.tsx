// React Flow custom node rendering a file as a card: left color stripe
// (drives from the user's per-node color override if set), bold file
// name, size chip, and a short list of top-level symbols. A hover-only
// palette button opens the shared NodeColorPicker.

import { memo } from "react";
import { Handle, Position, type NodeProps } from "@xyflow/react";
import * as Popover from "@radix-ui/react-popover";
import { NODE_COLORS, type NodeLabel } from "./constants";
import { NodeColorPicker } from "./NodeColorPicker";
import type { FileNodeData } from "./mermaidGraphBuilder";

const DEFAULT_STRIPE = "#3b82f6";

function formatBytes(bytes: number | null | undefined): string | null {
	if (bytes == null) return null;
	if (bytes < 1024) return `${bytes} B`;
	if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
	return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

interface ExtendedData extends FileNodeData {
	colorOverride?: string;
	onRecolorNode?: (fileQname: string, color: string | null) => void;
}

export const FileNodeCard = memo(({ data, selected }: NodeProps) => {
	const { file, symbols, colorOverride, onRecolorNode } = data as ExtendedData;
	const stripeColor = colorOverride ?? DEFAULT_STRIPE;
	const size = formatBytes(file.file_size);
	const displayName = file.name.length > 28 ? `${file.name.slice(0, 27)}…` : file.name;

	return (
		<div
			className={
				"group relative min-w-[200px] max-w-[240px] overflow-hidden rounded-lg border bg-app-darkBox text-ink shadow-[0_2px_8px_rgba(0,0,0,0.4)] transition-[box-shadow,border-color] " +
				(selected
					? "border-accent"
					: "border-app-line hover:border-app-line/80")
			}
		>
			<Handle type="target" position={Position.Top} className="!h-1.5 !w-1.5 !border-0 !bg-ink-faint opacity-40" />
			<span
				aria-hidden
				className="absolute inset-y-0 left-0 w-1"
				style={{ background: stripeColor }}
			/>
			<div className="pl-3 pr-2 py-2">
				<div className="flex items-center justify-between gap-2">
					<span
						className="truncate text-sm font-medium text-ink"
						title={file.source_file ?? file.name}
					>
						{displayName}
					</span>
					<div className="flex shrink-0 items-center gap-1">
						{size && (
							<span className="font-mono text-[9px] text-ink-faint">{size}</span>
						)}
						{onRecolorNode && (
							<Popover.Root>
								<Popover.Trigger asChild>
									<button
										className="nodrag rounded p-0.5 text-ink-faint/0 transition-all group-hover:text-ink-faint hover:!bg-app-hover hover:!text-ink"
										title="Change card color"
										onClick={(e) => e.stopPropagation()}
									>
										<svg viewBox="0 0 16 16" fill="currentColor" className="h-3 w-3">
											<path d="M13.4 1.6a2.1 2.1 0 0 0-3 0L3.3 8.7a1 1 0 0 0-.2.4l-1 3.5a.5.5 0 0 0 .6.6l3.5-1a1 1 0 0 0 .4-.2l7.1-7.1a2.1 2.1 0 0 0 0-3ZM11 3.2l1.8 1.8-5.7 5.7-2.3.6.6-2.3Z" />
										</svg>
									</button>
								</Popover.Trigger>
								<Popover.Portal>
									<Popover.Content
										side="right"
										sideOffset={6}
										className="z-50 rounded-lg border border-app-line bg-app-darkBox p-2 shadow-xl"
										onClick={(e) => e.stopPropagation()}
									>
										<NodeColorPicker
											currentColor={colorOverride ?? DEFAULT_STRIPE}
											defaultColor={DEFAULT_STRIPE}
											onSelect={(c) => onRecolorNode(file.qualified_name, c)}
											onReset={() => onRecolorNode(file.qualified_name, null)}
										/>
									</Popover.Content>
								</Popover.Portal>
							</Popover.Root>
						)}
					</div>
				</div>
				{symbols.length > 0 && (
					<ul className="mt-1.5 space-y-0.5">
						{symbols.map((sym) => (
							<li key={sym.qualified_name} className="flex items-center gap-1.5 text-[10px] text-ink-dull">
								<span
									className="h-1.5 w-1.5 shrink-0 rounded-full"
									style={{ background: NODE_COLORS[sym.label as NodeLabel] ?? "#64748b" }}
								/>
								<span className="truncate">{sym.name}</span>
							</li>
						))}
					</ul>
				)}
			</div>
			<Handle type="source" position={Position.Bottom} className="!h-1.5 !w-1.5 !border-0 !bg-ink-faint opacity-40" />
		</div>
	);
});

FileNodeCard.displayName = "FileNodeCard";
