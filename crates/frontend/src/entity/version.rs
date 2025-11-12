use std::sync::{atomic::{AtomicBool, Ordering}, Arc};

use bridge::{handle::BackendHandle, message::MessageToBackend};
use gpui::{AppContext, Entity};
use schema::version_manifest::MinecraftVersionManifest;

pub struct VersionEntries {
    pub manifest: Option<Result<Arc<MinecraftVersionManifest>, Arc<str>>>,
    sent_initial_load: AtomicBool,
    pending_reload: AtomicBool,
    backend_handle: BackendHandle
}

impl VersionEntries {
    pub fn new(backend_handle: BackendHandle) -> Self {
        Self {
            manifest: None,
            sent_initial_load: AtomicBool::new(false),
            pending_reload: AtomicBool::new(false),
            backend_handle
        }
    }

    pub fn set<C: AppContext>(entity: &Entity<Self>, manifest: Result<Arc<MinecraftVersionManifest>, Arc<str>>, cx: &mut C) {
        entity.update(cx, |entries, cx| {
            entries.pending_reload.store(false, Ordering::Relaxed);
            entries.manifest = Some(manifest);
            cx.notify();
        });
    }

    pub fn load_if_missing(&self) {
        if !self.sent_initial_load.swap(true, Ordering::Relaxed) {
            self.backend_handle.blocking_send(MessageToBackend::LoadVersionManifest {
                reload: false
            });
        }
    }

    pub fn reload(&self) {
        if !self.pending_reload.swap(true, Ordering::Relaxed) {
            self.sent_initial_load.store(true, Ordering::Relaxed);

            self.backend_handle.blocking_send(MessageToBackend::LoadVersionManifest {
                reload: true
            });
        }
    }
}
