use std::{collections::HashSet, path::Path, sync::Arc};

use bridge::instance::InstanceID;
use notify::{event::{CreateKind, DataChange, ModifyKind, RemoveKind, RenameMode}, EventKind};

use crate::{BackendState, WatchTarget};

#[derive(Debug)]
enum FilesystemEvent {
    Change(Arc<Path>),
    Remove(Arc<Path>),
    Rename(Arc<Path>, Arc<Path>),
}

impl FilesystemEvent {
    pub fn change_or_remove_path(&self) -> Option<&Arc<Path>> {
        match self {
            FilesystemEvent::Change(path) => Some(path),
            FilesystemEvent::Remove(path) => Some(path),
            FilesystemEvent::Rename(..) => None,
        }
    }
}

struct AfterDebounceEffects {
    reload_mods: HashSet<InstanceID>,
}

impl BackendState {
    pub async fn handle_filesystem(&mut self, result: notify_debouncer_full::DebounceEventResult) {
        match result {
            Ok(events) => {
                let mut after_debounce_effects = AfterDebounceEffects {
                    reload_mods: HashSet::new(),
                };
                
                let mut last_event: Option<FilesystemEvent> = None;
                for event in events {
                    let Some(next_event) = get_simple_event(event.event) else {
                        continue;
                    };
                    
                    if let Some(last_event) = last_event.take() {
                        if last_event.change_or_remove_path() != next_event.change_or_remove_path() {
                            self.handle_filesystem_event(last_event, &mut after_debounce_effects).await;
                        }
                    }
                    
                    last_event = Some(next_event);
                }
                if let Some(last_event) = last_event.take() {
                    self.handle_filesystem_event(last_event, &mut after_debounce_effects).await;
                }
                for id in after_debounce_effects.reload_mods {
                    if let Some(instance) = self.instances.get_mut(id.index) {
                        if instance.id == id {
                            instance.start_load_mods(&self.notify_tick, &self.mod_metadata_manager);
                        }
                    }
                }
            },
            Err(_) => {
                eprintln!("An error occurred while watching the filesystem! The launcher might be out-of-sync with your files!");
                self.send.send_error("An error occurred while watching the filesystem! The launcher might be out-of-sync with your files!").await;
            },
        }
    }
    
    async fn handle_filesystem_change_event(&mut self, path: Arc<Path>, after_debounce_effects: &mut AfterDebounceEffects) {
        if let Some(target) = self.watching.get(&path).copied() {
            if self.filesystem_handle_change(target, &path, after_debounce_effects).await {
                return;
            }
        }
        let Some(parent_path) = path.parent() else {
            return;
        };
        if let Some(parent) = self.watching.get(parent_path).copied() {
            self.filesystem_handle_child_change(parent, parent_path, &path, after_debounce_effects).await;
        }
    }
    
    async fn handle_filesystem_remove_event(&mut self, path: Arc<Path>, after_debounce_effects: &mut AfterDebounceEffects) {
        if let Some(target) = self.watching.remove(&path) {
            if self.filesystem_handle_removed(target, &path, after_debounce_effects).await {
                return;
            }
        }
        let Some(parent_path) = path.parent() else {
            return;
        };
        if let Some(parent) = self.watching.get(parent_path).copied() {
            self.filesystem_handle_child_removed(parent, parent_path, &path, after_debounce_effects).await;
        }
    }
    
    async fn handle_filesystem_rename_event(&mut self, from: Arc<Path>, to: Arc<Path>, after_debounce_effects: &mut AfterDebounceEffects) {
        if let Some(target) = self.watching.remove(&from) {
            if self.filesystem_handle_renamed(target, &from, &to, after_debounce_effects).await {
                return;
            }
        }
        if let Some(parent_path) = from.parent() {
            if let Some(parent) = self.watching.get(parent_path).copied() {
                if self.filesystem_handle_child_renamed(parent, parent_path, &from, &to, after_debounce_effects).await {
                    return;
                }
            }
        }
        self.handle_filesystem_remove_event(from, after_debounce_effects).await;
        self.handle_filesystem_change_event(to, after_debounce_effects).await;
    }
    
    async fn handle_filesystem_event(&mut self, event: FilesystemEvent, after_debounce_effects: &mut AfterDebounceEffects) {
        match event {
            FilesystemEvent::Change(path) => self.handle_filesystem_change_event(path, after_debounce_effects).await,
            FilesystemEvent::Remove(path) => self.handle_filesystem_remove_event(path, after_debounce_effects).await,
            FilesystemEvent::Rename(from, to) => self.handle_filesystem_rename_event(from, to, after_debounce_effects).await,
        }
    }
    
    async fn filesystem_handle_change(&mut self, target: WatchTarget, _path: &Arc<Path>, _after_debounce_effects: &mut AfterDebounceEffects) -> bool {
        match target {
            WatchTarget::ServersDat { id } => {
                if let Some(instance) = self.instances.get_mut(id.index) && instance.id == id {
                    instance.mark_servers_dirty();
                }
                true
            },
            _ => false
        }
    }
    
    async fn filesystem_handle_removed(&mut self, target: WatchTarget, path: &Arc<Path>, _after_debounce_effects: &mut AfterDebounceEffects) -> bool {
        match target {
            WatchTarget::InstancesDir => {
                self.send.send_error("Instances folder has been removed! What?!").await;
                true
            },
            WatchTarget::InstanceDir { id } => {
                self.remove_instance(id).await;
                true
            },
            WatchTarget::InvalidInstanceDir => {
                true
            },
            WatchTarget::InstanceWorldDir { id } => {
                if let Some(instance) = self.instances.get_mut(id.index) && instance.id == id {
                    instance.mark_world_dirty(Some(path.clone()));
                }
                true
            },
            WatchTarget::InstanceSavesDir { id } => {
                if let Some(instance) = self.instances.get_mut(id.index) && instance.id == id {
                    instance.mark_world_dirty(None);
                }
                true
            },
            WatchTarget::ServersDat { id } => {
                if let Some(instance) = self.instances.get_mut(id.index) && instance.id == id {
                    instance.mark_servers_dirty();
                }
                // Minecraft moves the servers.dat to servers.dat_old and then back,
                // so lets just re-listen immediately
                if self.watcher.watch(&path, notify::RecursiveMode::NonRecursive).is_ok() {
                    self.watching.insert(path.clone(), target);
                }
                true
            },
            WatchTarget::InstanceModsDir { id } => {
                if let Some(instance) = self.instances.get_mut(id.index) && instance.id == id {
                    instance.mark_mods_dirty(None);
                }
                true
            },
            WatchTarget::InstanceDotMinecraftDir { id } => {
                if let Some(instance) = self.instances.get_mut(id.index) && instance.id == id {
                    instance.mark_world_dirty(None);
                    instance.mark_servers_dirty();
                    instance.mark_mods_dirty(None);
                }
                true
            },
        }
    }
    
    async fn filesystem_handle_renamed(&mut self, from_target: WatchTarget, from: &Arc<Path>, to: &Arc<Path>, _after_debounce_effects: &mut AfterDebounceEffects) -> bool {
        match from_target {
            WatchTarget::InstanceDir { id } => {
                if let Some(instance) = self.instances.get_mut(id.index) && instance.id == id {
                    if from.parent() == to.parent() {
                        let old_name = instance.name.clone();
                        instance.name = to.file_name().unwrap().to_string_lossy().into_owned().into();
                        
                        self.send.send_info(format!("Instance '{}' renamed to '{}'", old_name, instance.name)).await;
                        self.send.send(instance.create_modify_message()).await;
                        
                        let watching_dot_minecraft = instance.watching_dot_minecraft;
                        let watching_saves_dir = instance.watching_saves_dir;
                        let watching_server_dat = instance.watching_server_dat;
                        let watching_mods_dir = instance.watching_mods_dir;
                        let dot_minecraft_path = instance.dot_minecraft_path.clone();
                        let saves_path = instance.saves_path.clone();
                        let server_dat_path = instance.server_dat_path.clone();
                        let mods_path = instance.mods_path.clone();
                        
                        self.watch_filesystem(&to, WatchTarget::InstanceDir { id }).await;
                        if watching_dot_minecraft {
                            self.watch_filesystem(&dot_minecraft_path, WatchTarget::InstanceDotMinecraftDir { id }).await;
                        }
                        if watching_saves_dir {
                            self.watch_filesystem(&saves_path, WatchTarget::InstanceSavesDir { id }).await;
                        }
                        if watching_server_dat {
                            self.watch_filesystem(&server_dat_path, WatchTarget::ServersDat { id }).await;
                        }
                        if watching_mods_dir {
                            self.watch_filesystem(&mods_path, WatchTarget::InstanceModsDir { id }).await;
                        }
                        
                        return true;
                    }
                }
                false
            },
            _ => false
        }
    }
    
    async fn filesystem_handle_child_change(&mut self, parent: WatchTarget, parent_path: &Path, path: &Arc<Path>, after_debounce_effects: &mut AfterDebounceEffects) {
        match parent {
            WatchTarget::InstancesDir => {
                if path.is_dir() {
                    let success = self.load_instance_from_path(&path, false, true).await;
                    if !success {
                        self.watch_filesystem(&path, WatchTarget::InvalidInstanceDir).await;
                    }
                }
            },
            WatchTarget::InstanceDir { id } => {
                let Some(file_name) = path.file_name() else {
                    return;
                };
                if file_name == "info_v1.json" {
                    self.load_instance_from_path(parent_path, true, true).await;
                } else if file_name == ".minecraft" {
                    if let Some(instance) = self.instances.get_mut(id.index) && instance.id == id {
                        instance.mark_world_dirty(None);
                        instance.mark_servers_dirty();
                        instance.mark_mods_dirty(None);
                        
                        let watching_dot_minecraft = instance.watching_dot_minecraft;
                        let watching_saves_dir = instance.watching_saves_dir;
                        let watching_server_dat = instance.watching_server_dat;
                        let watching_mods_dir = instance.watching_mods_dir;
                        let saves_path = instance.saves_path.clone();
                        let server_dat_path = instance.server_dat_path.clone();
                        let mods_path = instance.mods_path.clone();
                        
                        if watching_dot_minecraft {
                            self.watch_filesystem(&path, WatchTarget::InstanceDotMinecraftDir { id }).await;
                        }
                        if watching_saves_dir {
                            self.watch_filesystem(&saves_path, WatchTarget::InstanceSavesDir { id }).await;
                        }
                        if watching_server_dat {
                            self.watch_filesystem(&server_dat_path, WatchTarget::ServersDat { id }).await;
                        }
                        if watching_mods_dir {
                            self.watch_filesystem(&mods_path, WatchTarget::InstanceModsDir { id }).await;
                        }
                    }
                }
            },
            WatchTarget::ServersDat { .. } => {},
            WatchTarget::InstanceDotMinecraftDir { id } => {
                let Some(file_name) = path.file_name() else {
                    return;
                };
                if let Some(instance) = self.instances.get_mut(id.index) && instance.id == id {
                    match file_name.to_str() {
                        Some("mods") if instance.watching_mods_dir => {
                            instance.mark_mods_dirty(None);
                            self.watch_filesystem(&path, WatchTarget::InstanceModsDir { id }).await;
                        },
                        Some("saves") if instance.watching_saves_dir => {
                            instance.mark_world_dirty(None);
                            self.watch_filesystem(&path, WatchTarget::InstanceSavesDir { id }).await;
                        },
                        Some("servers.dat") if instance.watching_server_dat => {
                            instance.mark_servers_dirty();
                            self.watch_filesystem(&path, WatchTarget::ServersDat { id }).await;
                        },
                        _ => {}
                    }
                }
            }
            WatchTarget::InvalidInstanceDir => {
                let Some(file_name) = path.file_name() else {
                    return;
                };
                if file_name == "info_v1.json" {
                    self.load_instance_from_path(parent_path, true, true).await;
                }
            },
            WatchTarget::InstanceWorldDir { id } => {
                // If a file inside the world folder is changed (e.g. icon.png), mark the world (parent) as dirty
                if let Some(instance) = self.instances.get_mut(id.index) && instance.id == id {
                    instance.mark_world_dirty(Some(parent_path.into()));
                }
            },
            WatchTarget::InstanceSavesDir { id } => {
                // If a world folder is added to the saves directory, mark the world (path) as dirty
                if let Some(instance) = self.instances.get_mut(id.index) && instance.id == id {
                    instance.mark_world_dirty(Some(path.clone()));
                }
            },
            WatchTarget::InstanceModsDir { id } => {
                if let Some(instance) = self.instances.get_mut(id.index) && instance.id == id {
                    instance.mark_mods_dirty(Some(path.clone()));
                    if let Some(reload_immediately) = self.reload_mods_immediately.take(&instance.id) {
                        after_debounce_effects.reload_mods.insert(reload_immediately);
                    }
                }
            },
        }
    }
    
    async fn filesystem_handle_child_removed(&mut self, parent: WatchTarget, parent_path: &Path, path: &Arc<Path>, after_debounce_effects: &mut AfterDebounceEffects) {
        match parent {
            WatchTarget::InstanceDir { id } => {
                let Some(file_name) = path.file_name() else {
                    return;
                };
                if file_name == "info_v1.json" {
                    self.remove_instance(id).await;
                    self.watch_filesystem(parent_path, WatchTarget::InvalidInstanceDir).await;
                }
            },
            WatchTarget::InstanceWorldDir { id } => {
                if let Some(instance) = self.instances.get_mut(id.index) && instance.id == id {
                    instance.mark_world_dirty(Some(parent_path.into()));
                }
            },
            WatchTarget::InstanceSavesDir { id } => {
                if let Some(instance) = self.instances.get_mut(id.index) && instance.id == id {
                    instance.mark_world_dirty(Some(path.clone()));
                }
            },
            WatchTarget::InstanceModsDir { id } => {
                if let Some(instance) = self.instances.get_mut(id.index) && instance.id == id {
                    instance.mark_mods_dirty(Some(path.clone()));
                    if let Some(reload_immediately) = self.reload_mods_immediately.take(&instance.id) {
                        after_debounce_effects.reload_mods.insert(reload_immediately);
                    }
                }
            }
            _ => {}
        }
    }
    
    async fn filesystem_handle_child_renamed(&mut self, _parent: WatchTarget, _parent_path: &Path, _from: &Arc<Path>, _to: &Arc<Path>, _after_debounce_effects: &mut AfterDebounceEffects) -> bool {
        false
    }
    
}

fn get_simple_event(event: notify::Event) -> Option<FilesystemEvent> {
    match event.kind {
        EventKind::Create(create_kind) => {
            if create_kind == CreateKind::Other {
                return None;
            }
            return Some(FilesystemEvent::Change(event.paths[0].clone().into()));
        },
        EventKind::Modify(modify_kind) => {
            match modify_kind {
                ModifyKind::Any => {
                    return Some(FilesystemEvent::Change(event.paths[0].clone().into()));
                },
                ModifyKind::Data(data_change) => {
                    if data_change == DataChange::Any || data_change == DataChange::Content {
                        return Some(FilesystemEvent::Change(event.paths[0].clone().into()));
                    } else {
                        return None;
                    }
                },
                ModifyKind::Metadata(_) => return None,
                ModifyKind::Name(rename_mode) => {
                    match rename_mode {
                        RenameMode::Any => return None,
                        RenameMode::To => {
                            return Some(FilesystemEvent::Change(event.paths[0].clone().into()));
                        },
                        RenameMode::From => {
                            return Some(FilesystemEvent::Remove(event.paths[0].clone().into()));
                        },
                        RenameMode::Both => {
                            return Some(FilesystemEvent::Rename(event.paths[0].clone().into(), event.paths[1].clone().into()));
                        },
                        RenameMode::Other => return None,
                    }
                },
                ModifyKind::Other => return None,
            }
        },
        EventKind::Remove(remove_kind) => {
            if remove_kind == RemoveKind::Other {
                return None;
            }
    
            return Some(FilesystemEvent::Remove(event.paths[0].clone().into()));
        },
        EventKind::Any => return None,
        EventKind::Access(_) => return None,
        EventKind::Other => return None,
    };
}
