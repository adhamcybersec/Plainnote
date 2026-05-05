<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!--
	SigmaWrapper.svelte — wraps Sigma.js + graphology in a Svelte 5
	$effect lifecycle. Same imperative pattern as CodeMirrorWrapper:
	the parent provides node/edge data; the wrapper owns the renderer
	and exposes a thin imperative API. Never bind reactive state into
	Sigma's internal graph — Sigma's graph IS the source of truth for
	the rendering.

	Layout coordinates come from the Rust force-directed layout (M5-T2).
	The wrapper does NOT recompute layout — it renders coordinates verbatim.
-->
<script lang="ts" module>
	import type { GraphData, NoteId } from '$lib/ipc';
	export interface SigmaRef {
		fitView: () => void;
		focus: (id: NoteId) => void;
	}
</script>

<script lang="ts">
	import Graph from 'graphology';
	import Sigma from 'sigma';

	let {
		data,
		onNodeClick = (_id: NoteId) => {}
	} = $props<{
		data: GraphData;
		onNodeClick?: (id: NoteId) => void;
	}>();

	let host: HTMLDivElement | undefined = $state();
	let renderer: Sigma | undefined;
	let graph: Graph | undefined;

	$effect(() => {
		if (!host) return;
		// Build graphology graph from the Rust-supplied data.
		graph = new Graph();
		for (const n of data.nodes) {
			graph.addNode(n.id, {
				label: n.title || '(untitled)',
				size: n.size,
				x: n.x,
				y: n.y,
				color: '#5e7c82'
			});
		}
		for (const e of data.edges) {
			// Skip degenerate or duplicate edges defensively — Rust already
			// dedupes, but graphology throws on conflicts.
			if (e.source === e.target) continue;
			if (graph.hasEdge(e.source, e.target)) continue;
			if (!graph.hasNode(e.source) || !graph.hasNode(e.target)) continue;
			graph.addEdge(e.source, e.target, { color: '#33363c' });
		}

		renderer = new Sigma(graph, host, {
			renderLabels: true,
			labelDensity: 1,
			labelGridCellSize: 60,
			labelRenderedSizeThreshold: 6,
			defaultNodeColor: '#5e7c82',
			defaultEdgeColor: '#33363c'
		});

		renderer.on('clickNode', ({ node }) => {
			onNodeClick(node);
		});

		return () => {
			renderer?.kill();
			renderer = undefined;
			graph = undefined;
		};
	});

	export function fitView(): void {
		if (!renderer) return;
		const camera = renderer.getCamera();
		camera.animatedReset();
	}

	export function focus(id: NoteId): void {
		if (!renderer || !graph || !graph.hasNode(id)) return;
		const attrs = graph.getNodeAttributes(id);
		const camera = renderer.getCamera();
		camera.animate(
			{ x: attrs.x as number, y: attrs.y as number, ratio: 0.3 },
			{ duration: 250 }
		);
	}
</script>

<div bind:this={host} class="pn-sigma" data-testid="sigma-host"></div>
