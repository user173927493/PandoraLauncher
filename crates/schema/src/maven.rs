use std::sync::Arc;

use serde::Deserialize;
use ustr::Ustr;


#[derive(Debug, Deserialize)]
#[serde(rename = "metadata")]
pub struct MavenMetadataXml {
    // #[serde(rename = "groupId")]
    // pub group_id: Arc<str>,
    // #[serde(rename = "artifactId")]
    // pub artifact_id: Arc<str>,
    pub versioning: MavenMetadataVersioning,
}


#[derive(Debug, Deserialize)]
#[serde(rename = "versioning")]
pub struct MavenMetadataVersioning {
    // pub latest: Arc<str>,
    // pub release: Arc<str>,
    pub versions: MavenMetadataVersions,
}

#[derive(Debug, Deserialize)]
pub struct MavenMetadataVersions {
    #[serde(rename = "version")]
    pub version: Arc<[Ustr]>,
}
