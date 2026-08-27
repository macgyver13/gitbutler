use crate::diff::worktree_changes::repo;
use anyhow::Result;
use but_core::submodule::submodule_status;
use snapbox::prelude::*;

#[test]
fn submodule_on_gitbutler_workspace_head() -> Result<()> {
    let repo = repo("submodule-in-gitbutler-workspace")?;
    let actual =
        submodule_status(&repo, "submodule".into())?.expect("the submodule is active and cloned");

    // The workspace commit is never pushed, so recording it would leave the superproject with a
    // gitlink nobody else can resolve. Its parents are the stack tips, one of which reached the
    // remote and one of which did not.
    snapbox::assert_data_eq!(
        actual.to_debug(),
        snapbox::str![[r#"
SubmoduleStatus {
    path: "submodule",
    head: Sha1(3c1235a924bc6cb5ffa9a8657cefa12b646aab4e),
    head_ref: Some(
        FullName(
            "refs/heads/gitbutler/workspace",
        ),
    ),
    head_is_workspace_commit: true,
    head_is_pushed: false,
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
    let actual =
        submodule_status(&repo, "submodule".into())?.expect("the submodule is active and cloned");

    assert!(
        !actual.head_is_workspace_commit,
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
        submodule_status(&repo, "not-a-submodule".into())?.is_none(),
        "paths that are not active submodules yield no information rather than an error"
    );
    Ok(())
}
