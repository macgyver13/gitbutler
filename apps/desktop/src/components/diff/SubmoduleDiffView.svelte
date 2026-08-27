<script lang="ts">
	import { CLIPBOARD_SERVICE } from "$lib/backend/clipboard";
	import { inject } from "@gitbutler/core/context";
	import { CopyButton, Tooltip } from "@gitbutler/ui";
	import type { TreeChange, TreeStatus } from "@gitbutler/but-sdk";

	type Props = {
		change: TreeChange;
	};

	const { change }: Props = $props();

	const clipboardService = inject(CLIPBOARD_SERVICE);

	/**
	 * The commit the submodule pointed at before and after the change. Submodules are
	 * gitlinks, so both sides are commit ids rather than blobs, and either side may be
	 * absent when the submodule was added or removed.
	 */
	function submoduleCommits(status: TreeStatus): { previous?: string; current?: string } {
		switch (status.type) {
			case "Addition":
				return { current: status.subject.state.id };
			case "Deletion":
				return { previous: status.subject.previousState.id };
			case "Modification":
			case "Rename":
				return {
					previous: status.subject.previousState.id,
					current: status.subject.state.id,
				};
		}
	}

	const commits = $derived(submoduleCommits(change.status));
</script>

<div class="submodule">
	<p class="text-12 text-semibold submodule__title">Submodule</p>
	<p class="text-12 submodule__path">{change.path}</p>

	<div class="submodule__commits">
		{#if commits.previous}
			{@const previous = commits.previous}
			<Tooltip text="Copy previous commit SHA">
				<CopyButton
					text={previous.slice(0, 7)}
					hideIcon
					onclick={() => {
						clipboardService.write(previous, { message: "Commit SHA copied" });
					}}
				/>
			</Tooltip>
		{/if}
		{#if commits.previous && commits.current}
			<span class="submodule__arrow">→</span>
		{/if}
		{#if commits.current}
			{@const current = commits.current}
			<Tooltip text="Copy current commit SHA">
				<CopyButton
					text={current.slice(0, 7)}
					hideIcon
					onclick={() => {
						clipboardService.write(current, { message: "Commit SHA copied" });
					}}
				/>
			</Tooltip>
		{/if}
	</div>
</div>

<style lang="postcss">
	.submodule {
		display: flex;
		align-items: center;
		padding: 12px 14px;
		gap: 8px;
	}

	.submodule__title {
		color: var(--clr-text-2);
	}

	.submodule__path {
		flex: 1;
		overflow: hidden;
		color: var(--clr-text-1);
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.submodule__commits {
		display: flex;
		align-items: center;
		gap: 4px;
		font-family: var(--font-mono);
	}

	.submodule__arrow {
		color: var(--clr-text-2);
	}
</style>
