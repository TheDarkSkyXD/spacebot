// Footer bar for the Code Graph tab. Shows the loaded node/edge counts
// and a "layout running" indicator.

interface Props {
	nodeCount: number;
	edgeCount: number;
	isLayoutRunning: boolean;
	isUpdating?: boolean;
	truncated: boolean;
}

export function CodeGraphStatusBar({
	nodeCount,
	edgeCount,
	isLayoutRunning,
	isUpdating,
	truncated,
}: Props) {
	return (
		<div className="flex shrink-0 items-center justify-between border-t border-app-line bg-app-darkBox px-4 py-1.5 text-[11px] text-ink-faint">
			<div className="flex items-center gap-4">
				<span>
					<span className="text-ink-dull">{nodeCount.toLocaleString()}</span> nodes
				</span>
				<span>
					<span className="text-ink-dull">{edgeCount.toLocaleString()}</span> edges
				</span>
				{truncated && (
					<span className="text-amber-400">truncated</span>
				)}
			</div>
			<div className="flex items-center gap-2">
				{isUpdating && (
					<>
						<span className="h-1.5 w-1.5 animate-pulse rounded-full bg-blue-500" />
						<span className="text-blue-400">Updating graph...</span>
					</>
				)}
				{isLayoutRunning && !isUpdating && (
					<>
						<span className="h-1.5 w-1.5 animate-pulse rounded-full bg-emerald-500" />
						<span className="text-emerald-400">Layout optimizing...</span>
					</>
				)}
			</div>
		</div>
	);
}
