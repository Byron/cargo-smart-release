use std::process::Stdio;

use anyhow::anyhow;
use cargo_metadata::{
    camino::{Utf8Component, Utf8Path},
    Dependency, DependencyKind, Metadata, Package, PackageId,
};
use gix::bstr::{BStr, ByteSlice};
use semver::{Version, VersionReq};

pub struct Program {
    pub found: bool,
}

impl Program {
    pub fn named(name: &'static str) -> Self {
        Program {
            found: std::process::Command::new(name)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok(),
        }
    }
}

pub fn will(not_really: bool) -> &'static str {
    if not_really {
        "WOULD"
    } else {
        "Will"
    }
}

pub fn try_to_published_crate_and_new_version<'meta, 'a>(
    c: &'a crate::traverse::Dependency<'meta>,
) -> Option<(&'meta Package, &'a semver::Version)> {
    match &c.mode {
        crate::traverse::dependency::Mode::ToBePublished { adjustment } => {
            Some((c.package, &adjustment.bump().next_release))
        }
        _ => None,
    }
}

pub fn is_pre_release_version(semver: &Version) -> bool {
    semver.major == 0
}

pub fn is_top_level_package(manifest_path: &Utf8Path, repo: &gix::Repository) -> bool {
    manifest_path
        .strip_prefix(
            std::env::current_dir()
                .expect("cwd")
                .join(repo.work_dir().as_ref().expect("repo with working tree")),
        )
        .is_ok_and(|p| p.components().count() == 1)
}

pub fn version_req_unset_or_default(req: &VersionReq) -> bool {
    req.comparators.last().is_none_or(|comp| comp.op == semver::Op::Caret)
}

pub fn package_eq_dependency_ignore_dev_without_version(package: &Package, dependency: &Dependency) -> bool {
    (dependency.kind != DependencyKind::Development || !version_req_unset_or_default(&dependency.req))
        && package.name == dependency.name
}

pub fn workspace_package_by_dependency<'a>(meta: &'a Metadata, dep: &Dependency) -> Option<&'a Package> {
    meta.packages
        .iter()
        .find(|p| p.name == dep.name && p.source.as_ref().is_none_or(|s| !s.is_crates_io()))
        .filter(|p| meta.workspace_members.iter().any(|m| m == &p.id))
}

pub fn package_by_name<'a>(meta: &'a Metadata, name: &str) -> anyhow::Result<&'a Package> {
    meta.packages
        .iter()
        .find(|p| p.name == name && p.source.as_ref().is_none_or(|s| !s.is_crates_io()))
        .ok_or_else(|| anyhow!("workspace member '{}' must be a listed package", name))
}

pub fn names_and_versions<'a>(publishees: impl IntoIterator<Item = &'a (&'a Package, &'a semver::Version)>) -> String {
    publishees
        .into_iter()
        .map(|(p, nv)| format!("{} v{}", p.name, nv))
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn package_by_id<'a>(meta: &'a Metadata, id: &PackageId) -> &'a Package {
    meta.packages
        .iter()
        .find(|p| &p.id == id)
        .expect("workspace members are in packages")
}

pub fn tag_prefix<'p>(package: &'p Package, repo: &gix::Repository) -> Option<&'p str> {
    if is_top_level_package(&package.manifest_path, repo) {
        None
    } else {
        Some(&package.name)
    }
}

pub fn tag_name(package: &Package, version: &semver::Version, repo: &gix::Repository) -> String {
    tag_name_inner(tag_prefix(package, repo), version)
}

fn tag_name_inner(package_name: Option<&str>, version: &semver::Version) -> String {
    match package_name {
        Some(name) => format!("{name}-v{version}"),
        None => format!("v{version}"),
    }
}

pub fn parse_possibly_prefixed_tag_version(package_name: Option<&str>, tag_name: &BStr) -> Option<Version> {
    match package_name {
        Some(name) => tag_name
            .strip_prefix(name.as_bytes())
            .and_then(|r| r.strip_prefix(b"-"))
            .and_then(|possibly_version| parse_tag_version(possibly_version.as_bstr())),
        None => parse_tag_version(tag_name),
    }
}

pub fn parse_tag_version(name: &BStr) -> Option<Version> {
    let version = name
        .strip_prefix(b"vers")
        .or_else(|| name.strip_prefix(b"v"))
        .unwrap_or_else(|| name.as_bytes())
        .to_str()
        .ok()?;
    Version::parse(version).ok()
}

pub fn is_tag_name(package_name: &str, tag_name: &gix::bstr::BStr) -> bool {
    match tag_name
        .strip_prefix(package_name.as_bytes())
        .and_then(|r| r.strip_prefix(b"-"))
    {
        None => false,
        Some(possibly_version) => parse_tag_version(possibly_version.as_bstr()).is_some(),
    }
}

pub fn is_tag_version(name: &gix::bstr::BStr) -> bool {
    parse_tag_version(name).is_some()
}

pub fn component_to_bytes(c: Utf8Component<'_>) -> &[u8] {
    match c {
        Utf8Component::Normal(c) => c.as_bytes(),
        _ => unreachable!("only normal components are possible in paths here"),
    }
}

pub fn time_to_zoned_time(time: gix::date::Time) -> anyhow::Result<jiff::Zoned> {
    Ok(jiff::Timestamp::new(time.seconds, 0)?.to_zoned(jiff::tz::Offset::from_seconds(time.offset)?.to_time_zone()))
}

#[cfg(test)]
mod tests {
    mod parse_possibly_prefixed_tag_version {
        mod matches {
            use std::str::FromStr;

            use gix::bstr::ByteSlice;
            use semver::Version;

            use crate::utils::{parse_possibly_prefixed_tag_version, tag_name_inner};

            #[test]
            fn whatever_tag_name_would_return() {
                assert_eq!(
                    parse_possibly_prefixed_tag_version(
                        "git-test".into(),
                        tag_name_inner("git-test".into(), &Version::from_str("1.0.1").unwrap())
                            .as_bytes()
                            .as_bstr()
                    ),
                    Version::parse("1.0.1").expect("valid").into()
                );

                assert_eq!(
                    parse_possibly_prefixed_tag_version(
                        "single".into(),
                        tag_name_inner("single".into(), &Version::from_str("0.0.1-beta.1").unwrap())
                            .as_bytes()
                            .as_bstr()
                    ),
                    Version::parse("0.0.1-beta.1").expect("valid").into()
                );

                assert_eq!(
                    parse_possibly_prefixed_tag_version(
                        None,
                        tag_name_inner(None, &Version::from_str("0.0.1+123.x").unwrap())
                            .as_bytes()
                            .as_bstr()
                    ),
                    Version::parse("0.0.1+123.x").expect("valid").into()
                );
            }
        }
    }

    mod is_tag_name {
        mod no_match {
            use std::str::FromStr;

            use gix::bstr::ByteSlice;
            use semver::Version;

            use crate::utils::{is_tag_name, tag_name_inner};

            #[test]
            fn due_to_crate_name() {
                assert!(!is_tag_name(
                    "foo",
                    tag_name_inner("bar".into(), &Version::from_str("0.0.1-beta.1").unwrap())
                        .as_bytes()
                        .as_bstr()
                ));
            }
        }
        mod matches {
            use std::str::FromStr;

            use gix::bstr::ByteSlice;
            use semver::Version;

            use crate::utils::{is_tag_name, tag_name_inner};

            #[test]
            fn whatever_tag_name_would_return() {
                assert!(is_tag_name(
                    "git-test",
                    tag_name_inner("git-test".into(), &Version::from_str("1.0.1").unwrap())
                        .as_bytes()
                        .as_bstr()
                ));

                assert!(is_tag_name(
                    "single",
                    tag_name_inner("single".into(), &Version::from_str("0.0.1-beta.1").unwrap())
                        .as_bytes()
                        .as_bstr()
                ));
            }
        }
    }
    mod is_tag_version {
        mod no_match {
            use gix::bstr::ByteSlice;

            use crate::utils::is_tag_version;

            #[test]
            fn not_enough_numbers() {
                assert!(!is_tag_version(b"v0.0".as_bstr()));
            }

            #[test]
            fn funky() {
                assert!(!is_tag_version(b"vHi.Ho.yada-anythingreally".as_bstr()));
            }

            #[test]
            fn prefixed() {
                assert!(!is_tag_version(b"cargo-v1.0.0".as_bstr()));
            }
        }
        mod matches {
            use gix::bstr::ByteSlice;

            #[test]
            fn no_prefix() {
                assert!(is_tag_version(b"0.0.1".as_bstr()));
            }

            #[test]
            fn custom_prefix() {
                assert!(is_tag_version(b"vers0.0.1".as_bstr()));
            }

            use crate::utils::is_tag_version;

            #[test]
            fn pre_release() {
                assert!(is_tag_version(b"v0.0.1".as_bstr()));
                assert!(is_tag_version(b"v0.10.0-beta.1".as_bstr()));
            }

            #[test]
            fn production() {
                assert!(is_tag_version(b"v1.0.1-alpha.1".as_bstr()));
                assert!(is_tag_version(b"v18.10.0+meta".as_bstr()));
            }
        }
    }
}
