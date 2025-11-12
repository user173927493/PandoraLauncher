use std::{collections::HashMap, fmt::Display, path::{Path, PathBuf}, sync::Arc};

use bridge::{handle::FrontendHandle, message::MessageToFrontend};
use schema::{assets_index::AssetsIndex, fabric_launch::FabricLaunch, fabric_loader_manifest::{FabricLoaderManifest, FABRIC_LOADER_MANIFEST_URL}, java_runtime_component::JavaRuntimeComponentManifest, java_runtimes::{JavaRuntimes, JAVA_RUNTIMES_URL}, version::MinecraftVersion, version_manifest::{MinecraftVersionLink, MinecraftVersionManifest, MOJANG_VERSION_MANIFEST_URL}};
use serde::de::DeserializeOwned;
use sha1::{Digest, Sha1};
use tokio::{runtime::Handle, task::JoinHandle};
use ustr::Ustr;

type MetaLoadStateWrapper<T> = Arc<tokio::sync::Mutex<MetaLoadState<T>>>;

#[derive(Default)]
pub struct MetadataManagerStates {
    minecraft_version_manifest: MetaLoadStateWrapper<MinecraftVersionManifest>,
    mojang_java_runtimes: MetaLoadStateWrapper<JavaRuntimes>,
    fabric_loader_manifest: MetaLoadStateWrapper<FabricLoaderManifest>,
    fabric_launch: std::sync::RwLock<HashMap<(Ustr, Ustr), MetaLoadStateWrapper<FabricLaunch>>>,
    version_info: std::sync::RwLock<HashMap<Ustr, MetaLoadStateWrapper<MinecraftVersion>>>,
    assets_index: std::sync::RwLock<HashMap<Ustr, MetaLoadStateWrapper<AssetsIndex>>>,
    java_runtime_manifests: std::sync::RwLock<HashMap<Ustr, MetaLoadStateWrapper<JavaRuntimeComponentManifest>>>,
}

pub struct MetadataManager {
    states: MetadataManagerStates,

    metadata_cache: PathBuf,
    version_manifest_cache: Arc<Path>,
    mojang_java_runtimes_cache: Arc<Path>,
    fabric_loader_manifest_cache: Arc<Path>,

    http_client: reqwest::Client,
    sender: FrontendHandle,
    runtime: Handle
}

pub trait MetadataItem {
    type T: DeserializeOwned + Send + Sync + 'static;

    fn url(&self) -> Ustr;
    fn state(&self, states: &MetadataManagerStates) -> MetaLoadStateWrapper<Self::T>;
    fn cache_file(&self, _metadata_manager: &MetadataManager) -> impl AsRef<Path> + Send + Sync + 'static;
    fn data_hash(&self) -> Option<Ustr> {
        None
    }
    fn try_send_to_backend(_value: Result<Arc<Self::T>, MetaLoadError>, _sender: FrontendHandle) -> impl std::future::Future<Output = ()> + Send {
        async {}
    }
}

pub struct MinecraftVersionManifestMetadata;

impl MetadataItem for MinecraftVersionManifestMetadata {
    type T = MinecraftVersionManifest;

    fn url(&self) -> Ustr {
        MOJANG_VERSION_MANIFEST_URL.into()
    }

    fn cache_file(&self, metadata_manager: &MetadataManager) -> impl AsRef<Path> + Send + Sync + 'static {
        Arc::clone(&metadata_manager.version_manifest_cache)
    }

    fn state(&self, states: &MetadataManagerStates) -> MetaLoadStateWrapper<Self::T> {
        states.minecraft_version_manifest.clone()
    }

    async fn try_send_to_backend(value: Result<Arc<Self::T>, MetaLoadError>, sender: FrontendHandle) {
        let _ = sender.send(MessageToFrontend::VersionManifestUpdated(value.map_err(|err| format!("{}", err).into()))).await;
    }
}

pub struct MojangJavaRuntimesMetadata;

impl MetadataItem for MojangJavaRuntimesMetadata {
    type T = JavaRuntimes;

    fn url(&self) -> Ustr {
        JAVA_RUNTIMES_URL.into()
    }

    fn cache_file(&self, metadata_manager: &MetadataManager) -> impl AsRef<Path> + Send + Sync + 'static {
        Arc::clone(&metadata_manager.mojang_java_runtimes_cache)
    }

    fn state(&self, states: &MetadataManagerStates) -> MetaLoadStateWrapper<Self::T> {
        states.mojang_java_runtimes.clone()
    }
}

pub struct MinecraftVersionMetadata<'v>(pub &'v MinecraftVersionLink);

impl <'v> MetadataItem for MinecraftVersionMetadata<'v> {
    type T = MinecraftVersion;

    fn url(&self) -> Ustr {
        self.0.url
    }

    fn data_hash(&self) -> Option<Ustr> {
        Some(self.0.sha1)
    }

    fn cache_file(&self, metadata_manager: &MetadataManager) -> impl AsRef<Path> + Send + Sync + 'static {
        if !crate::is_single_component_path(&self.0.sha1) {
            panic!("Invalid sha1 {}, possible directory traversal attack?", self.0.sha1);
        }
        let mut path = metadata_manager.metadata_cache.join("version_info");
        path.push(self.0.sha1.as_str());
        path
    }

    fn state(&self, states: &MetadataManagerStates) -> MetaLoadStateWrapper<Self::T> {
        let url = self.url();
        if let Some(item) = states.version_info.read().unwrap().get(&url) {
            item.clone()
        } else {
            let mut write = states.version_info.write().unwrap();
            write.entry(url).or_default().clone()
        }
    }
}

pub struct AssetsIndexMetadata {
    pub url: Ustr,
    pub cache: Arc<Path>,
    pub hash: Ustr,
}

impl MetadataItem for AssetsIndexMetadata {
    type T = AssetsIndex;

    fn url(&self) -> Ustr {
        self.url
    }

    fn state(&self, states: &MetadataManagerStates) -> MetaLoadStateWrapper<Self::T> {
        let url = self.url();
        if let Some(item) = states.assets_index.read().unwrap().get(&url) {
            item.clone()
        } else {
            let mut write = states.assets_index.write().unwrap();
            write.entry(url).or_default().clone()
        }
    }

    fn cache_file(&self, _: &MetadataManager) -> impl AsRef<Path> + Send + Sync + 'static {
        Arc::clone(&self.cache)
    }
    
    fn data_hash(&self) -> Option<Ustr> {
        Some(self.hash)
    }
}

pub struct MojangJavaRuntimeComponentMetadata {
    pub url: Ustr,
    pub cache: Arc<Path>,
    pub hash: Ustr,
}

impl MetadataItem for MojangJavaRuntimeComponentMetadata {
    type T = JavaRuntimeComponentManifest;

    fn url(&self) -> Ustr {
        self.url
    }

    fn state(&self, states: &MetadataManagerStates) -> MetaLoadStateWrapper<Self::T> {
        let url = self.url();
        if let Some(item) = states.java_runtime_manifests.read().unwrap().get(&url) {
            item.clone()
        } else {
            let mut write = states.java_runtime_manifests.write().unwrap();
            write.entry(url).or_default().clone()
        }
    }

    fn cache_file(&self, _: &MetadataManager) -> impl AsRef<Path> + Send + Sync + 'static {
        Arc::clone(&self.cache)
    }
    
    fn data_hash(&self) -> Option<Ustr> {
        Some(self.hash)
    }
}

pub struct FabricLoaderManifestMetadata;

impl MetadataItem for FabricLoaderManifestMetadata {
    type T = FabricLoaderManifest;

    fn url(&self) -> Ustr {
        Ustr::from(FABRIC_LOADER_MANIFEST_URL)
    }

    fn cache_file(&self, metadata_manager: &MetadataManager) -> impl AsRef<Path> + Send + Sync + 'static {
        Arc::clone(&metadata_manager.fabric_loader_manifest_cache)
    }

    fn state(&self, states: &MetadataManagerStates) -> MetaLoadStateWrapper<Self::T> {
        states.fabric_loader_manifest.clone()
    }
}

pub struct FabricLaunchMetadata {
    pub minecraft_version: Ustr,
    pub loader_version: Ustr,
}

impl MetadataItem for FabricLaunchMetadata {
    type T = FabricLaunch;

    fn url(&self) -> Ustr {
        format!("https://meta.fabricmc.net/v2/versions/loader/{}/{}", self.minecraft_version, self.loader_version).into()
    }

    fn cache_file(&self, metadata_manager: &MetadataManager) -> impl AsRef<Path> + Send + Sync + 'static {
        let mut path = metadata_manager.metadata_cache.join("fabric_launch");
        path.push(self.minecraft_version.as_str());
        path.push(self.loader_version.as_str());
        path
    }

    fn state(&self, states: &MetadataManagerStates) -> MetaLoadStateWrapper<Self::T> {
        let key = (self.minecraft_version, self.loader_version);
        if let Some(item) = states.fabric_launch.read().unwrap().get(&key) {
            item.clone()
        } else {
            let mut write = states.fabric_launch.write().unwrap();
            write.entry(key).or_default().clone()
        }
    }
}

#[derive(thiserror::Error, Clone, Debug)]
pub enum MetaLoadError {
    InvalidHash,
    Reqwest(Arc<reqwest::Error>),
    SerdeJson(Arc<serde_json::Error>),
    TokioJoin(Arc<tokio::task::JoinError>),
}

impl Display for MetaLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidHash => {
                f.write_str("Data did not match expected hash")
            },
            Self::Reqwest(error) => {
                if let Some(url) = error.url() {
                    if error.is_connect() {
                        return f.write_fmt(format_args!("Unable to connect to {}", url));
                    } else if error.is_timeout() {
                        return f.write_fmt(format_args!("Connection to {} timed out", url));
                    } else if error.is_decode() {
                        return f.write_fmt(format_args!("Unable to decode response from {}", url));
                    } else if error.is_builder() {
                        return f.write_fmt(format_args!("Unexpected error while constructing request to {}", url));
                    }
                } else if error.is_connect() {
                    return f.write_str("Unable to connect");
                } else if error.is_timeout() {
                    return f.write_str("Connection timed out");
                } else if error.is_decode() {
                    return f.write_str("Unable to decode response");
                } else if error.is_builder() {
                    return f.write_str("Unexpected error while constructing request");
                }

                f.debug_tuple("Reqwest").field(error).finish()
            },
            Self::SerdeJson(_) => {
                f.write_str("Data was missing or malformed")
            }
            Self::TokioJoin(error) => f.debug_tuple("TokioJoin").field(error).finish(),
        }
    }
}

impl From<reqwest::Error> for MetaLoadError {
    fn from(error: reqwest::Error) -> Self {
        Self::Reqwest(Arc::new(error))
    }
}

impl From<serde_json::Error> for MetaLoadError {
    fn from(error: serde_json::Error) -> Self {
        Self::SerdeJson(Arc::new(error))
    }
}

impl From<tokio::task::JoinError> for MetaLoadError {
    fn from(error: tokio::task::JoinError) -> Self {
        Self::TokioJoin(Arc::new(error))
    }
}

#[derive(Default)]
pub enum MetaLoadState<T> {
    #[default]
    Unloaded,
    Pending(JoinHandle<Result<Arc<T>, MetaLoadError>>),
    Loaded(Arc<T>),
    Error(MetaLoadError)
}

impl MetadataManager {
    pub fn new(http_client: reqwest::Client, runtime: Handle, directory: PathBuf, sender: FrontendHandle) -> Self {
        Self {
            states: MetadataManagerStates::default(),

            version_manifest_cache: directory.join("version_manifest.json").into(),
            mojang_java_runtimes_cache: directory.join("mojang_java_runtimes.json").into(),
            fabric_loader_manifest_cache: directory.join("fabric_loader_manifest.json").into(),
            metadata_cache: directory,

            http_client,
            sender,
            runtime
        }
    }

    pub async fn load<I: MetadataItem>(&self, item: &I) {
        let state = item.state(&self.states);
        let mut state = state.lock().await;
        if matches!(*state, MetaLoadState::Unloaded) {
            let cache_file = item.cache_file(self);
            Self::inner_start_loading(&mut *state, item, Some(cache_file), &self.http_client, &self.runtime, self.sender.clone());
        }
    }

    pub async fn force_reload<I: MetadataItem>(&self, item: &I) {
        let state = item.state(&self.states);
        let mut state = state.lock().await;
        Self::inner_start_loading(&mut *state, item, None::<PathBuf>, &self.http_client, &self.runtime, self.sender.clone());
    }

    pub async fn fetch<I: MetadataItem>(&self, item: &I) -> Result<Arc<<I as MetadataItem>::T>, MetaLoadError> {
        let state = item.state(&self.states);
        let mut state = state.lock().await;
        let cache_file = item.cache_file(self);
        Self::inner_fetch(&mut *state, item, cache_file, &self.http_client, &self.runtime, self.sender.clone()).await
    }

    fn inner_start_loading<I: MetadataItem>(state: &mut MetaLoadState<I::T>, item: &I, cache_file: Option<impl AsRef<Path> + Send + Sync + 'static>, http_client: &reqwest::Client, runtime: &Handle, sender: FrontendHandle) {
        let url = item.url();
        let http_client = http_client.clone();
        let expected_hash = item.data_hash().and_then(|sha1| {
            let mut expected_hash = [0u8; 20];
            hex::decode_to_slice(sha1.as_str(), &mut expected_hash).ok()?;
            Some(expected_hash)
        });
        let join_handle = runtime.spawn(async move {
            let mut file_fallback = None;

            if let Some(cache_file) = &cache_file {
                let cache_file = cache_file.as_ref().to_owned();
                let meta = tokio::task::spawn_blocking(move || {
                    let Ok(file) = std::fs::read(&cache_file) else {
                        return None;
                    };
                    
                    let correct_hash = if let Some(expected_hash) = &expected_hash {
                        let mut hasher = Sha1::new();
                        hasher.update(&file);
                        let actual_hash = hasher.finalize();
                        
                        expected_hash == &*actual_hash
                    } else {
                        true
                    };
                    
                    if !correct_hash {
                        eprintln!("Sha1 mismatch for {:?}, downloading file again...", cache_file);
                        return None;
                    }

                    let result: Result<I::T, serde_json::Error> = serde_json::from_slice(&file);
                    match result {
                        Ok(meta) => {
                            Some(meta)
                        },
                        Err(error) => {
                            eprintln!("Error parsing cached metadata file for {:?}, downloading file again... {}", cache_file, error);
                            None
                        },
                    }
                }).await.unwrap();
                if let Some(meta) = meta {
                    if expected_hash.is_some() {
                        return Ok(Arc::new(meta));
                    } else {
                        file_fallback = Some(Arc::new(meta));
                    }
                }   
            }

            let url = &url;
            let mut result: Result<Arc<I::T>, MetaLoadError> = async move {
                let response = http_client.get(url.as_str()).timeout(std::time::Duration::from_secs(5)).send().await?;
                let bytes = response.bytes().await?;

                // We try to decode before checking the hash because it's a more
                // useful error message to know that the content is invalid
                let meta: I::T = serde_json::from_slice(&bytes)?;

                let correct_hash = if let Some(expected_hash) = &expected_hash {
                    let mut hasher = Sha1::new();
                    hasher.update(&bytes);
                    let actual_hash = hasher.finalize();

                    expected_hash == &*actual_hash
                } else {
                    true
                };

                if !correct_hash {
                    return Err(MetaLoadError::InvalidHash);
                }

                if let Some(cache_file) = &cache_file {
                    if let Some(parent) = cache_file.as_ref().parent() {
                        let _ = tokio::fs::create_dir_all(parent).await;
                    }
                    let _ = tokio::fs::write(cache_file, bytes).await;
                }

                Ok(Arc::new(meta))
            }.await;

            if let Err(error) = &result {
                if let Some(file_fallback) = file_fallback {
                    eprintln!("Error while fetching metadata {:?}, using file fallback: {error:?}", std::any::type_name::<I::T>());
                    result = Ok(file_fallback);
                } else {
                    eprintln!("Error while fetching metadata {:?}: {error:?}", std::any::type_name::<I::T>());
                }
            }

            I::try_send_to_backend(result.clone(), sender).await;

            result
        });

        *state = MetaLoadState::Pending(join_handle);
    }

     async fn inner_fetch<I: MetadataItem>(state: &mut MetaLoadState<I::T>, item: &I, cache_file: impl AsRef<Path> + Send + Sync + 'static, http_client: &reqwest::Client, runtime: &Handle, sender: FrontendHandle) -> Result<Arc<I::T>, MetaLoadError> {
        if let MetaLoadState::Unloaded = state {
            Self::inner_start_loading(state, item, Some(cache_file), http_client, runtime, sender);
        }

        match state {
            MetaLoadState::Unloaded => unreachable!(),
            MetaLoadState::Pending(join_handle) => {
                let result = join_handle.await.map_err(MetaLoadError::from).flatten();
                match result {
                    Ok(value) => {
                        *state = MetaLoadState::Loaded(Arc::clone(&value));
                        Ok(value)
                    },
                    Err(error) => {
                        *state = MetaLoadState::Error(error.clone());
                        Err(error)
                    },
                }
            },
            MetaLoadState::Loaded(value) => {
                Ok(Arc::clone(value))
            },
            MetaLoadState::Error(meta_load_error) => {
                Err(meta_load_error.clone())
            },
        }
    }
}
