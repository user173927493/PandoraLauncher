use std::sync::Arc;

use crate::instance::InstanceID;

#[derive(Debug, Clone, Copy)]
pub enum InstallTarget {
    Instance(InstanceID),
    Library,
    NewInstance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentType {
    Mod,
    Modpack,
    Resourcepack,
    Shader
}

#[derive(Debug, Clone)]
pub struct ContentInstall {
    pub target: InstallTarget,
    pub files: Arc<[ContentInstallFile]>
}

#[derive(Debug, Clone)]
pub struct ContentInstallFile {
    pub url: Arc<str>,
    pub filename: Arc<str>,
    pub sha1: Arc<str>,
    pub size: usize,
    pub content_type: ContentType,
}
