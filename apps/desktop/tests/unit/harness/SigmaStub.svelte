<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!--
	Test stub for SigmaWrapper. Stand-in component that renders a marker
	so route-level tests can assert "the wrapper mounted with data" without
	needing WebGL in jsdom.
-->
<script lang="ts">
	import type { GraphData, NoteId } from '$lib/ipc';
	let { data, onNodeClick = (_id: NoteId) => {} } = $props<{
		data: GraphData;
		onNodeClick?: (id: NoteId) => void;
	}>();

	function clickFirst() {
		if (data.nodes.length > 0) onNodeClick(data.nodes[0].id);
	}
</script>

<div data-testid="sigma-stub" data-node-count={data.nodes.length}>
	<button type="button" data-testid="stub-click-first" onclick={clickFirst}>
		click first node
	</button>
</div>
