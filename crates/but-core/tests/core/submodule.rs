use crate::diff::worktree_changes::repo;
use anyhow::Result;
use but_core::submodule::submodule_status;
use snapbox::prelude::*;

#[test]
fn submodule_on_gitbutler_workspace_head() -> Result<()> {
    let repo = repo("submodule-in-gitbutler-workspace")?;
    let actual = submodule_status(&repo, "submodule".into(), None)?
        .expect("the submodule is active and cloned");

    // The workspace commit is never pushed, so recording it would leave the superproject with a
    // gitlink nobody else can resolve. Its parents are the stack tips, one of which reached the
    // remote and one of which did not.
    snapbox::assert_data_eq!(
        actual.to_debug(),
        snapbox::str![[r#"
SubmoduleStatus {
    path: "submodule",
    commit: Sha1(3c1235a924bc6cb5ffa9a8657cefa12b646aab4e),
    is_head: true,
    head_ref: Some(
        FullName(
            "refs/heads/gitbutler/workspace",
        ),
    ),
    ref_name: None,
    is_workspace_commit: true,
    is_pushed: false,
    candidates: [
        SubmoduleCandidate {
            id: Sha1(9b9b027ac35c8ec628f6c37a999505e1b4e89f84),
            ref_name: Some(
                FullName(
                    "refs/heads/pushed-feature",
                ),
            ),
            is_pushed: true,
        },
        SubmoduleCandidate {
            id: Sha1(90586e457418a12f9a57d270cab32ac45e527a98),
            ref_name: Some(
                FullName(
                    "refs/heads/local-feature",
                ),
            ),
            is_pushed: false,
        },
    ],
}

"#]]
    );
    Ok(())
}

#[test]
fn submodule_on_an_ordinary_branch_reports_no_candidates() -> Result<()> {
    let repo = repo("submodule-changed-head")?;
    let actual = submodule_status(&repo, "submodule".into(), None)?
        .expect("the submodule is active and cloned");

    assert!(
        !actual.is_workspace_commit,
        "an ordinary checkout is not a workspace"
    );
    assert!(
        actual.candidates.is_empty(),
        "candidates are only offered to replace a workspace commit"
    );
    Ok(())
}

#[test]
fn an_embedded_repository_has_no_submodule_status() -> Result<()> {
    let repo = repo("submodule-changed-head")?;
    assert!(
        submodule_status(&repo, "not-a-submodule".into(), None)?.is_none(),
        "paths that are not active submodules yield no information rather than an error"
    );
    Ok(())
}

/// A gitlink recorded in an earlier commit is described by asking about that commit directly,
/// rather than by resolving the submodule's current `HEAD` which says nothing about it.
#[test]
fn a_recorded_workspace_commit_is_recognised() -> Result<()> {
    let repo = repo("submodule-in-gitbutler-workspace")?;
    let head = submodule_status(&repo, "submodule".into(), None)?.expect("active submodule");
    assert!(head.is_head, "with no commit given, HEAD is described");

    // Ask about the very same commit explicitly: it is still recognised as a workspace commit,
    // now by its subject rather than by the ref HEAD happens to be on.
    let recorded =
        submodule_status(&repo, "submodule".into(), Some(head.commit))?.expect("active submodule");
    assert!(
        recorded.is_workspace_commit,
        "a workspace commit is recognised by subject when reached as a plain id"
    );
    assert!(
        !recorded.is_pushed,
        "the workspace commit never reached a remote"
    );
    assert_eq!(
        recorded.candidates.len(),
        head.candidates.len(),
        "the same stack tips are offered either way"
    );

    // An ordinary commit reached the same way is not mistaken for a workspace.
    let tip = head.candidates.first().expect("a stack tip").id;
    let ordinary =
        submodule_status(&repo, "submodule".into(), Some(tip))?.expect("active submodule");
    assert!(!ordinary.is_head, "the tip is not what HEAD points at");
    assert!(!ordinary.is_workspace_commit);
    assert!(
        ordinary.is_pushed,
        "the first candidate is the branch that reached the remote"
    );
    assert_eq!(
        ordinary.ref_name.as_ref().map(|n| n.to_string()),
        Some("refs/heads/pushed-feature".to_string()),
        "the branch at that commit is named for display"
    );
    Ok(())
}
