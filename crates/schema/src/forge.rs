use std::{collections::HashMap, sync::Arc};

use serde::Deserialize;
use ustr::Ustr;

use crate::{maven::MavenMetadataXml, version::GameLibrary};

pub const NEOFORGE_INSTALLER_MAVEN_URL: &str = "https://maven.neoforged.net/releases/net/neoforged/neoforge/maven-metadata.xml";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForgeInstallProfile {
    pub minecraft: Arc<str>,
    pub json: Arc<str>,
    pub mirror_list: Arc<str>,
    pub data: HashMap<String, ForgeSidedData>,
    pub processors: Arc<[ForgeInstallProcessor]>,
    pub libraries: Arc<[GameLibrary]>
}

#[derive(Debug, Deserialize)]
#[cfg_attr(debug_assertions, serde(deny_unknown_fields))]
pub struct ForgeSidedData {
    pub client: Arc<str>,
    pub server: Arc<str>,
}

#[derive(Debug, Deserialize, PartialEq, Eq, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum ForgeSide {
    Client,
    Server,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(debug_assertions, serde(deny_unknown_fields))]
pub struct ForgeInstallProcessor {
    pub sides: Option<Arc<[ForgeSide]>>,
    pub jar: Arc<str>,
    pub classpath: Arc<[Arc<str>]>,
    pub args: Arc<[Ustr]>,
    pub outputs: Option<HashMap<String, String>>,
}


#[derive(PartialEq, Eq, PartialOrd, Ord)]
pub enum VersionFragment {
    Alpha,
    Beta,
    Snapshot,
    String(String),
    Number(usize),
}

impl VersionFragment {
    pub fn string_to_parts(version: &str) -> Vec<Self> {
        version.split(&['.', '-', '+'])
            .map(|v| {
                if let Ok(number) = v.parse::<usize>() {
                    VersionFragment::Number(number)
                } else if v.eq_ignore_ascii_case("alpha") {
                    VersionFragment::Alpha
                } else if v.eq_ignore_ascii_case("beta") {
                    VersionFragment::Beta
                } else if v.eq_ignore_ascii_case("snapshot") {
                    VersionFragment::Snapshot
                } else {
                    VersionFragment::String(v.into())
                }
            })
            .collect::<Vec<_>>()
    }
}

#[derive(Debug)]
pub struct ForgeMavenManifest(pub Vec<Ustr>);

#[derive(Debug)]
pub struct NeoforgeMavenManifest(pub Vec<Ustr>);
