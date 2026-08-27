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

/// What the superproject would record for the submodule at [`path`](Self::path), and whether
/// that pointer can be resolved by somebody who clones the superproject.
#[derive(Debug, Clone)]
pub struct SubmoduleStatus {
    /// The worktree-relative path of the submodule in the superproject.
    pub path: BString,
    /// The commit the submodule worktree points at, i.e. what a commit would record.
    pub head: gix::ObjectId,
    /// The reference `HEAD` is symbolic to, or `None` if `HEAD` is detached.
    pub head_ref: Option<gix::refs::FullName>,
    /// `true` if [`head`](Self::head) is a synthetic GitButler workspace commit, which is
    /// never pushed and thus can never be resolved by anybody else.
    pub head_is_workspace_commit: bool,
    /// `true` if [`head`](Self::head) is reachable from one of the submodule's remote tracking
    /// branches, which is what a fresh clone would be able to resolve.
    pub head_is_pushed: bool,
    /// Commits that could be recorded instead of [`head`](Self::head), most-preferred first.
    ///
    /// Only populated when [`head_is_workspace_commit`](Self::head_is_workspace_commit) is set,
    /// as the parents of a workspace commit are the tips of the applied stacks.
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
    /// `true` if committing this gitlink would record a pointer that nobody else can resolve.
    pub fn is_unresolvable(&self) -> bool {
        !self.head_is_pushed
    }
}

/// Return the status of the submodule at `rela_path` in `repo`, or `None` if there is no active
/// submodule with a local clone there.
///
/// `None` is the normal outcome for an embedded repository or a submodule that was never
/// initialized: there is nothing to inspect without network access, and the caller should treat
/// that as "no information" rather than an error.
pub fn submodule_status(
    repo: &gix::Repository,
    rela_path: &BStr,
) -> anyhow::Result<Option<SubmoduleStatus>> {
    let Some(sm_repo) = open_submodule(repo, rela_path)? else {
        return Ok(None);
    };

    let head = sm_repo.head()?;
    let head_ref = head.referent_name().map(|name| name.to_owned());
    // GitButler always keeps `HEAD` symbolic to the workspace ref while a workspace is applied,
    // so the ref name is a sufficient signal; a detached `HEAD` is never a workspace.
    let head_is_workspace_commit = head_ref
        .as_ref()
        .is_some_and(|name| crate::is_workspace_ref_name(name.as_ref()));

    let Some(head_id) = sm_repo.head_id().ok().map(|id| id.detach()) else {
        // An unborn submodule has nothing to record.
        return Ok(None);
    };

    let mut graph: Graph<'_, '_, graph::Commit<Flags>> = Graph::new(&sm_repo, None);
    let remote_tips = remote_tips(&sm_repo)?;

    let candidates = if head_is_workspace_commit {
        sm_repo
            .find_commit(head_id)?
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
        head_is_pushed: is_reachable_from_any(&sm_repo, head_id, &remote_tips, &mut graph)?,
        head: head_id,
        head_ref,
        head_is_workspace_commit,
        candidates,
    }))
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
