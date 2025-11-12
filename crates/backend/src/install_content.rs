use std::{io::Write, path::PathBuf, sync::Arc};

use bridge::{install::{ContentInstall, ContentInstallFile, ContentType}, message::MessageToFrontend, modal_action::{ModalAction, ProgressTracker}};
use sha1::{Digest, Sha1};
use tokio::io::AsyncWriteExt;

use crate::BackendState;

#[derive(thiserror::Error, Debug)]
pub enum ContentInstallError {
    #[error("Failed to download remote content")]
    Reqwest(#[from] reqwest::Error),
    #[error("Downloaded file had the wrong size")]
    WrongFilesize,
    #[error("Downloaded file had the wrong hash")]
    WrongHash,
    #[error("Hash isn't a valid sha1 hash:\n{0}")]
    InvalidHash(Arc<str>),
    #[error("Failed to perform I/O operation:\n{0}")]
    IoError(#[from] std::io::Error),
    #[error("Invalid filename:\n{0}")]
    InvalidFilename(Arc<str>),
}

impl BackendState {
    pub async fn install_content(&mut self, content: ContentInstall, modal_action: ModalAction) {
        // todo: check library for hash already on disk!
        
        for content_file in content.files.iter() {
            if !crate::is_single_component_path(&content_file.filename) {
                let error = ContentInstallError::InvalidFilename(content_file.filename.clone());
                modal_action.set_error_message(Arc::from(format!("{}", error).as_str()));
                modal_action.set_finished();
                return;
            }   
        }
        
        let mut tasks = Vec::new();
        
        for content_file in content.files.iter() {
            tasks.push(async {
                let mut expected_hash = [0u8; 20];
                let Ok(_) = hex::decode_to_slice(&*content_file.sha1, &mut expected_hash) else {
                    eprintln!("Content install has invalid sha1: {}", content_file.sha1);
                    return Err(ContentInstallError::InvalidHash(content_file.sha1.clone()));
                };
                
                let title = format!("Downloading {}", content_file.filename);
                let tracker = ProgressTracker::new(title.into(), self.send.clone());
                modal_action.trackers.push(tracker.clone());
                
                tracker.set_total(content_file.size);
                tracker.notify().await;
                
                let response = self.http_client.get(&*content_file.url).send().await?;
                
                let hash_folder = self.directories.content_library_dir.join(&content_file.sha1[..2]);
                let _ = tokio::fs::create_dir_all(&hash_folder).await;
                let path = hash_folder.join(&*content_file.sha1);
                let mut file = tokio::fs::File::create(&path).await?;
                
                use futures::StreamExt;
                let mut stream = response.bytes_stream();
                
                let mut total_bytes = 0;
                
                let mut hasher = Sha1::new();
                while let Some(item) = stream.next().await {
                    let item = item?;
                    
                    total_bytes += item.len();
                    tracker.add_count(item.len());
                    tracker.notify().await;
                    
                    hasher.write_all(&item)?;
                    file.write_all(&item).await?;
                }
                
                let actual_hash = hasher.finalize();
                
                if *actual_hash != expected_hash {
                    return Err(ContentInstallError::WrongHash);
                }
                
                if total_bytes != content_file.size {
                    return Err(ContentInstallError::WrongFilesize);
                }
                
                Ok((path, content_file.clone()))
            });
        }
        
        let result: Result<Vec<(PathBuf, ContentInstallFile)>, ContentInstallError> = futures::future::try_join_all(tasks).await;
        match result {
            Ok(files) => {
                let mut instance_dir = None;
                
                match content.target {
                    bridge::install::InstallTarget::Instance(instance_id) => {
                        if let Some(instance) = self.instances.get(instance_id.index) && instance.id == instance_id {
                            instance_dir = Some(instance.dot_minecraft_path.clone());
                        }
                    },
                    bridge::install::InstallTarget::Library => {},
                    bridge::install::InstallTarget::NewInstance => todo!(),
                }
                
                if let Some(instance_dir) = instance_dir {
                    for (path, content_file) in files {
                        let mut target_path = instance_dir.to_path_buf();
                        match content_file.content_type {
                            ContentType::Mod | ContentType::Modpack => {
                                target_path.push("mods");
                            },
                            ContentType::Resourcepack => {
                                target_path.push("resourcepacks");
                            },
                            ContentType::Shader => {
                                target_path.push("shaderpacks");
                            },
                        }
                        let _ = tokio::fs::create_dir_all(&target_path).await;
                        target_path.push(&*content_file.filename);
                        
                        let _ = tokio::fs::hard_link(path, target_path).await;
                    }
                }
            },
            Err(error) => {
                modal_action.set_error_message(Arc::from(format!("{}", error).as_str()));
            },
        }
        
        modal_action.set_finished();
        self.send.send(MessageToFrontend::Refresh).await;
    }
}
