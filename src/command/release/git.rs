use std::{convert::TryInto, process::Command};

use anyhow::{anyhow, bail};
use cargo_metadata::Package;
use gix::{bstr::ByteSlice, refs, refs::transaction::PreviousValue, Id};

use super::{tag_name, Options};
use crate::utils::will;

pub(in crate::command::release_impl) fn commit_changes(
    message: impl AsRef<str>,
    dry_run: bool,
    empty_commit_possible: bool,
    signoff: bool,
    ctx: &crate::Context,
) -> anyhow::Result<Option<Id<'_>>> {
    // TODO: replace with gitoxide one day
    let mut cmd = Command::new("git");
    cmd.arg("commit").arg("-am").arg(message.as_ref());
    if empty_commit_possible {
        cmd.arg("--allow-empty");
    }
    if signoff {
        cmd.arg("--signoff");
    }
    log::trace!("{} run {:?}", will(dry_run), cmd);
    if dry_run {
        return Ok(None);
    }

    if !cmd.status()?.success() {
        bail!("Failed to commit changed manifests");
    }
    Ok(Some(ctx.repo.find_reference("HEAD")?.peel_to_id_in_place()?))
}

pub(in crate::command::release_impl) fn create_version_tag<'repo>(
    publishee: &Package,
    new_version: &semver::Version,
    commit_id: Option<Id<'repo>>,
    tag_message: Option<String>,
    ctx: &'repo crate::Context,
    Options { dry_run, skip_tag, .. }: Options,
) -> anyhow::Result<Option<refs::FullName>> {
    if skip_tag {
        return Ok(None);
    }
    let tag_name = tag_name(publishee, new_version, &ctx.repo);
    if dry_run {
        match tag_message {
            Some(message) => {
                log::trace!(
                    "WOULD create tag object {} with changelog message, first line is: '{}'",
                    tag_name,
                    message.lines().next().unwrap_or("")
                );
            }
            None => {
                log::trace!("WOULD create tag {}", tag_name);
            }
        }
        Ok(Some(format!("refs/tags/{tag_name}").try_into()?))
    } else {
        let target = commit_id.expect("set in --execute mode");
        let constraint = PreviousValue::Any;
        let tag = match tag_message {
            Some(message) => {
                let tag = ctx.repo.tag(
                    tag_name,
                    target,
                    gix::objs::Kind::Commit,
                    Some(crate::git::author()?.to_ref()),
                    message,
                    constraint,
                )?;
                log::info!("Created tag object {} with release notes.", tag.name().as_bstr());
                tag
            }
            None => {
                let tag = ctx.repo.tag_reference(tag_name, target, constraint)?;
                log::info!("Created tag {}", tag.name().as_bstr());
                tag
            }
        };
        Ok(Some(tag.inner.name))
    }
}

// TODO: Use gitoxide here
pub fn push_tags_and_head(
    repo: &gix::Repository,
    tag_names: &[refs::FullName],
    Options { dry_run, skip_push, .. }: Options,
) -> anyhow::Result<()> {
    if skip_push || tag_names.is_empty() {
        return Ok(());
    }

    let mut cmd = Command::new("git");
    cmd.arg("push")
        .arg(
            repo.head()?
                .into_remote(gix::remote::Direction::Push)
                .ok_or_else(|| anyhow!("Cannot push in uninitialized repo"))??
                .name()
                .expect("configured remotes have a name")
                .as_bstr()
                .to_string(),
        )
        .arg("HEAD");
    for tag_name in tag_names {
        cmd.arg(tag_name.as_bstr().to_str()?);
    }

    log::trace!("{} run {:?}", will(dry_run), cmd);
    if dry_run || cmd.status()?.success() {
        Ok(())
    } else {
        bail!("'git push' invocation failed. Try to push manually and repeat the smart-release invocation to resume, possibly with --skip-push.");
    }
}

#[cfg(test)]
mod tests {
    use log::Level;

    use super::*;

    #[test]
    #[ignore = "TBD: isolate properly, worked in PR, but stopped working in CI"]
    fn test_commit_changes() {
        let ctx = crate::Context::new(
            vec![],
            false,
            crate::version::BumpSpec::Auto,
            crate::version::BumpSpec::Auto,
        )
        .unwrap();
        let message = "commit message";
        testing_logger::setup();
        let _ = commit_changes(message, true, false, false, &ctx).unwrap();
        testing_logger::validate(|captured_logs| {
            assert_eq!(captured_logs.len(), 1);
            assert_eq!(
                captured_logs[0].body,
                "WOULD run \"git\" \"commit\" \"-am\" \"commit message\""
            );
            assert_eq!(captured_logs[0].level, Level::Trace);
        });
    }

    #[test]
    #[ignore = "TBD: isolate properly, worked in PR, but stopped working in CI"]
    fn test_commit_changes_with_signoff() {
        let ctx = crate::Context::new(
            vec![],
            false,
            crate::version::BumpSpec::Auto,
            crate::version::BumpSpec::Auto,
        )
        .unwrap();
        let message = "commit message";
        testing_logger::setup();
        let _ = commit_changes(message, true, false, true, &ctx).unwrap();
        testing_logger::validate(|captured_logs| {
            assert_eq!(captured_logs.len(), 1);
            assert_eq!(
                captured_logs[0].body,
                "WOULD run \"git\" \"commit\" \"-am\" \"commit message\" \"--signoff\""
            );
            assert_eq!(captured_logs[0].level, Level::Trace);
        });
    }
}
