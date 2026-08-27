//! Inspection of submodules, i.e. tree entries of kind [`Commit`](gix::object::tree::EntryKind::Commit).
//!
//! The superproject records the commit its submodule worktree currently points at. When that
//! commit cannot be resolved from a fresh clone of the submodule, the recorded gitlink is
//! useless to everybody but the person who created it. The most common way to end up in that
//! state is a submodule that is itself managed by GitButler: its `HEAD` is symbolic to
//! `refs/heads/gitbutler/workspace`, which points at a synthetic commit that is never pushed.

use anyhow::Context;
use bstr::{BStr, BString};
use gix::revision::plumbing::{graph, merge_base::Flags};
use gix::revwalk::Graph;

/// A description of one commit of the submodule at [`path`](Self::path): the one a gitlink
/// records, or the one it would record, and whether somebody cloning the superproject could
/// resolve it.
#[derive(Debug, Clone)]
pub struct SubmoduleStatus {
    /// The worktree-relative path of the submodule in the superproject.
    pub path: BString,
    /// The commit being described.
    pub commit: gix::ObjectId,
    /// `true` if [`commit`](Self::commit) is what the submodule worktree currently points at,
    /// i.e. what committing this gitlink would record.
    pub is_head: bool,
    /// The reference `HEAD` is symbolic to, set only when describing `HEAD` and it isn't detached.
    pub head_ref: Option<gix::refs::FullName>,
    /// The local branch pointing at [`commit`](Self::commit), for display. GitButler's own
    /// bookkeeping refs are never reported here.
    pub ref_name: Option<gix::refs::FullName>,
    /// `true` if [`commit`](Self::commit) is a synthetic GitButler workspace commit, which is
    /// never pushed and thus can never be resolved by anybody else.
    pub is_workspace_commit: bool,
    /// `true` if [`commit`](Self::commit) is reachable from one of the submodule's remote
    /// tracking branches, which is what a fresh clone would be able to resolve.
    pub is_pushed: bool,
    /// Commits that could be recorded instead of [`commit`](Self::commit), most-preferred first.
    ///
    /// Only populated when [`is_workspace_commit`](Self::is_workspace_commit) is set, as the
    /// parents of a workspace commit are the tips of the applied stacks.
    pub candidates: Vec<SubmoduleCandidate>,
}

/// A commit that could be recorded as the gitlink instead of the submodule's current `HEAD`.
#[derive(Debug, Clone)]
pub struct SubmoduleCandidate {
    /// The commit to record.
    pub id: gix::ObjectId,
    /// The local branch pointing at [`id`](Self::id), for display. GitButler's own bookkeeping
    /// refs are never reported here.
    pub ref_name: Option<gix::refs::FullName>,
    /// `true` if [`id`](Self::id) is reachable from one of the submodule's remote tracking
    /// branches. A candidate that is not pushed is no more resolvable than the workspace commit.
    pub is_pushed: bool,
}

impl SubmoduleStatus {
    /// `true` if this gitlink is a pointer nobody else can resolve.
    pub fn is_unresolvable(&self) -> bool {
        !self.is_pushed
    }
}

/// The subject GitButler gives the synthetic commit at the base of a workspace.
///
/// `but_workspace::commit` and `but_graph::projection` keep their own copies to stay off each
/// other; change all of them together.
const WORKSPACE_COMMIT_TITLES: &[&str] =
    &["GitButler Workspace Commit", "GitButler Integration Commit"];

/// Return the status of the submodule at `rela_path` in `repo`, or `None` if there is no active
/// submodule with a local clone there.
///
/// `None` is the normal outcome for an embedded repository or a submodule that was never
/// initialized: there is nothing to inspect without network access, and the caller should treat
/// that as "no information" rather than an error.
pub fn submodule_status(
    repo: &gix::Repository,
    rela_path: &BStr,
    commit: Option<gix::ObjectId>,
) -> anyhow::Result<Option<SubmoduleStatus>> {
    let Some(sm_repo) = open_submodule(repo, rela_path)? else {
        return Ok(None);
    };

    let head = sm_repo.head()?;
    let head_id = sm_repo.head_id().ok().map(|id| id.detach());
    let Some(commit_id) = commit.or(head_id) else {
        // An unborn submodule has nothing to describe.
        return Ok(None);
    };
    let is_head = Some(commit_id) == head_id;

    // The symbolic ref only says something about the commit when that commit *is* `HEAD`.
    let head_ref = is_head
        .then(|| head.referent_name().map(|name| name.to_owned()))
        .flatten();
    // While a workspace is applied GitButler keeps `HEAD` symbolic to the workspace ref, which is
    // the cheap signal. A commit reached any other way (a historical gitlink, a detached `HEAD`)
    // has to be recognised by the subject GitButler gives it.
    let is_workspace_commit = head_ref
        .as_ref()
        .is_some_and(|name| crate::is_workspace_ref_name(name.as_ref()))
        || has_workspace_commit_subject(&sm_repo, commit_id)?;

    let mut graph: Graph<'_, '_, graph::Commit<Flags>> = Graph::new(&sm_repo, None);
    let remote_tips = remote_tips(&sm_repo)?;

    let candidates = if is_workspace_commit {
        sm_repo
            .find_commit(commit_id)?
            .parent_ids()
            .map(|parent| {
                let id = parent.detach();
                Ok(SubmoduleCandidate {
                    ref_name: local_branch_at(&sm_repo, id)?,
                    is_pushed: is_reachable_from_any(&sm_repo, id, &remote_tips, &mut graph)?,
                    id,
                })
            })
            .collect::<anyhow::Result<Vec<_>>>()?
    } else {
        Vec::new()
    };

    Ok(Some(SubmoduleStatus {
        path: rela_path.to_owned(),
        is_pushed: is_reachable_from_any(&sm_repo, commit_id, &remote_tips, &mut graph)?,
        ref_name: local_branch_at(&sm_repo, commit_id)?,
        commit: commit_id,
        is_head,
        head_ref,
        is_workspace_commit,
        candidates,
    }))
}

/// Return `true` if `commit_id` carries the subject GitButler gives its workspace commits.
///
/// A commit that is missing from the submodule is not an error: a gitlink recorded from another
/// clone may simply not be present here, and "not a workspace commit" is the right answer.
fn has_workspace_commit_subject(
    repo: &gix::Repository,
    commit_id: gix::ObjectId,
) -> anyhow::Result<bool> {
    let Ok(commit) = repo.find_commit(commit_id) else {
        return Ok(false);
    };
    let message = commit.message()?;
    let title = message.title.trim_ascii();
    Ok(WORKSPACE_COMMIT_TITLES
        .iter()
        .any(|known| title == known.as_bytes()))
}

/// Open the submodule at `rela_path`, or return `None` if it is not an active submodule or has
/// no local clone yet.
fn open_submodule(
    repo: &gix::Repository,
    rela_path: &BStr,
) -> anyhow::Result<Option<gix::Repository>> {
    Ok(repo
        .submodules()?
        .into_iter()
        .flatten()
        .find_map(|sm| {
            let is_active = sm.is_active().ok()?;
            is_active.then(|| -> anyhow::Result<_> {
                Ok(
                    if sm.path().ok().is_some_and(|sm_path| sm_path == rela_path) {
                        sm.open()?
                    } else {
                        None
                    },
                )
            })
        })
        .transpose()?
        .flatten())
}

/// Return the commits all remote tracking branches point at.
fn remote_tips(repo: &gix::Repository) -> anyhow::Result<Vec<gix::ObjectId>> {
    Ok(repo
        .references()?
        .remote_branches()?
        .filter_map(Result::ok)
        .filter_map(|mut r| r.peel_to_id().ok().map(|id| id.detach()))
        .collect())
}

/// Return the local branch pointing at `id`, ignoring GitButler's own bookkeeping refs as those
/// are never meaningful to record in a superproject.
fn local_branch_at(
    repo: &gix::Repository,
    id: gix::ObjectId,
) -> anyhow::Result<Option<gix::refs::FullName>> {
    for mut reference in repo.references()?.local_branches()?.filter_map(Result::ok) {
        let name = reference.name().to_owned();
        if name.as_bstr().starts_with(b"refs/heads/gitbutler/") {
            continue;
        }
        if reference.peel_to_id().is_ok_and(|tip| tip.detach() == id) {
            return Ok(Some(name));
        }
    }
    Ok(None)
}

/// Return `true` if `id` is an ancestor of (or equal to) any commit in `tips`.
fn is_reachable_from_any(
    repo: &gix::Repository,
    id: gix::ObjectId,
    tips: &[gix::ObjectId],
    graph: &mut Graph<'_, '_, graph::Commit<Flags>>,
) -> anyhow::Result<bool> {
    for tip in tips {
        if *tip == id {
            return Ok(true);
        }
        match repo.merge_base_with_graph(id, *tip, graph) {
            Ok(merge_base) => {
                if merge_base == id {
                    return Ok(true);
                }
            }
            Err(gix::repository::merge_base_with_graph::Error::NotFound { .. }) => continue,
            Err(err) => {
                return Err(err).context("Could not determine whether a commit was pushed");
            }
        }
    }
    Ok(false)
}
