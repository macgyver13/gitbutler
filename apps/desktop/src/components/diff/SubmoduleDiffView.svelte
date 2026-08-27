<script lang="ts">
	import ReduxResult from "$components/shared/ReduxResult.svelte";
	import { CLIPBOARD_SERVICE } from "$lib/backend/clipboard";
	import { DIFF_SERVICE } from "$lib/hunks/diffService.svelte";
	import { UI_STATE } from "$lib/state/uiState.svelte";
	import { inject } from "@gitbutler/core/context";
	import { Button, CopyButton, InfoMessage, Tooltip } from "@gitbutler/ui";
	import type { SelectionId } from "$lib/selection/key";
	import type { SubmoduleStatus, TreeChange, TreeStatus } from "@gitbutler/but-sdk";

	type Props = {
		projectId: string;
		change: TreeChange;
		selectionId: SelectionId;
	};

	const { projectId, change, selectionId }: Props = $props();

	const clipboardService = inject(CLIPBOARD_SERVICE);
	const diffService = inject(DIFF_SERVICE);
	const uiState = inject(UI_STATE);
	const projectState = $derived(uiState.project(projectId));

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

	// Only an uncommitted change can still have its recorded commit chosen. A committed one is
	// settled, so it gets the description without the picker.
	const isUncommitted = $derived(selectionId.type === "worktree");
	const statusQuery = $derived(
		commits.current
			? diffService.getSubmoduleStatus(projectId, change, commits.current)
			: undefined,
	);
	const status = $derived(statusQuery?.response ?? undefined);

	const override = $derived(
		isUncommitted ? projectState.submoduleCommitOverrides.current[change.path] : undefined,
	);

	function shortRef(refName: string | null | undefined): string | undefined {
		return refName?.replace("refs/heads/", "");
	}

	/**
	 * The branch to name beside the recorded sha: the chosen tip once the user picks one,
	 * otherwise whatever branch the recorded commit is the tip of.
	 */
	const currentRefName = $derived(
		override
			? shortRef(status?.candidates.find((candidate) => candidate.id === override)?.refName)
			: shortRef(status?.refName),
	);

	function setOverride(commitId: string | undefined) {
		const next = { ...projectState.submoduleCommitOverrides.current };
		if (commitId === undefined) {
			delete next[change.path];
		} else {
			next[change.path] = commitId;
		}
		projectState.submoduleCommitOverrides.set(next);
	}
</script>

{#snippet sha(id: string, label: string)}
	<Tooltip text="Copy {label} commit SHA">
		<CopyButton
			text={id.slice(0, 7)}
			hideIcon
			onclick={() => {
				clipboardService.write(id, { message: "Commit SHA copied" });
			}}
		/>
	</Tooltip>
{/snippet}

<div class="submodule">
	<div class="submodule__summary">
		<p class="text-12 text-semibold submodule__title">Submodule</p>
		<p class="text-12 submodule__path">{change.path}</p>

		<div class="submodule__commits">
			{#if commits.previous}
				{@render sha(commits.previous, "previous")}
			{/if}
			{#if commits.previous && commits.current}
				<span class="submodule__arrow">→</span>
			{/if}
			{#if commits.current}
				{@render sha(override ?? commits.current, "current")}
				{#if currentRefName}
					<span class="submodule__ref">{currentRefName}</span>
				{:else if status?.isWorkspaceCommit}
					<span class="submodule__ref">GitButler workspace</span>
				{/if}
			{/if}
		</div>
	</div>

	{#if statusQuery}
		<ReduxResult {projectId} result={statusQuery.result}>
			{#snippet children(status: SubmoduleStatus | null)}
				{#if status && !status.isPushed}
					<div class="submodule__warning">
						<InfoMessage filled outlined={false} style="warning">
							{#snippet title()}
								{#if status.isWorkspaceCommit}
									{isUncommitted ? "This points" : "This commit points"} at a GitButler workspace commit
								{:else}
									{isUncommitted ? "This points" : "This commit points"} at an unpushed commit
								{/if}
							{/snippet}
							{#snippet content()}
								<p>
									{#if status.isWorkspaceCommit}
										A GitButler workspace commit is never pushed, so
										{isUncommitted ? "committing this records" : "this records"} a commit nobody else
										can resolve after cloning.
									{:else}
										This commit has not reached the submodule's remote, so the pointer cannot be
										resolved after cloning until it is pushed.
									{/if}
								</p>
								{#if isUncommitted && status.candidates.length > 0}
									<p class="submodule__candidates-intro">Select a branch tip to record instead:</p>
									<div class="submodule__candidates">
										{#each status.candidates as candidate (candidate.id)}
											<Button
												kind={override === candidate.id ? "solid" : "outline"}
												style="gray"
												size="tag"
												onclick={() =>
													setOverride(override === candidate.id ? undefined : candidate.id)}
											>
												{candidate.refName?.replace("refs/heads/", "") ?? candidate.id.slice(0, 7)}
												{#if !candidate.isPushed}
													(unpushed)
												{/if}
											</Button>
										{/each}
									</div>
									{#if override}
										<p class="submodule__chosen">
											Committing will record {currentRefName
												? `${currentRefName} (${override.slice(0, 7)})`
												: override.slice(0, 7)} for this submodule.
										</p>
									{/if}
								{/if}
							{/snippet}
						</InfoMessage>
					</div>
				{/if}
			{/snippet}
		</ReduxResult>
	{/if}
</div>

<style lang="postcss">
	.submodule {
		display: flex;
		flex-direction: column;
		padding: 12px 14px;
		gap: 12px;
	}

	.submodule__summary {
		display: flex;
		align-items: center;
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

	.submodule__ref {
		max-width: 24ch;
		overflow: hidden;
		color: var(--clr-text-2);
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.submodule__candidates-intro {
		margin-top: 8px;
	}

	.submodule__candidates {
		display: flex;
		flex-wrap: wrap;
		margin-top: 6px;
		gap: 6px;
	}

	.submodule__chosen {
		margin-top: 8px;
		color: var(--clr-text-2);
	}
</style>
