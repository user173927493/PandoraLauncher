use std::sync::{atomic::{AtomicUsize, Ordering}, Arc};

use bridge::{handle::BackendHandle, instance::{InstanceID, InstanceModSummary}, message::{AtomicBridgeDataLoadState, MessageToBackend}};
use gpui::{prelude::*, *};
use gpui_component::{
    button::{Button, ButtonVariants}, h_flex, list::{ListDelegate, ListItem, ListState}, switch::Switch, v_flex, ActiveTheme as _, Icon, IconName, IndexPath, Sizable
};

use crate::{entity::instance::InstanceEntry, png_render_cache};

pub struct InstanceModsSubpage {
    instance: InstanceID,
    backend_handle: BackendHandle,
    mods_state: Arc<AtomicBridgeDataLoadState>,
    mod_list: Entity<ListState<ModsListDelegate>>,
}

impl InstanceModsSubpage {
    pub fn new(instance: &Entity<InstanceEntry>, backend_handle: BackendHandle, window: &mut gpui::Window, cx: &mut gpui::Context<Self>) -> Self {
        let instance = instance.read(cx);
        let instance_id = instance.id;
        
        let mods_state = Arc::clone(&instance.mods_state);
        
        let mods_list_delegate = ModsListDelegate {
            id: instance_id,
            backend_handle: backend_handle.clone(),
            mods: instance.mods.read(cx).to_vec(),
            searched: instance.mods.read(cx).to_vec(),
            confirming_delete: Arc::new(AtomicUsize::new(0))
        };
        
        let mods = instance.mods.clone();
        
        let mod_list = cx.new(move |cx| {
            cx.observe(&mods, |list: &mut ListState<ModsListDelegate>, mods, cx| {
                let mods = mods.read(cx).to_vec();
                let delegate = list.delegate_mut();
                delegate.mods = mods.clone();
                delegate.searched = mods;
                delegate.confirming_delete.store(0, Ordering::Release);
                cx.notify();
            }).detach();
            
            ListState::new(mods_list_delegate, window, cx).selectable(false).searchable(true)
        });
        
        Self {
            instance: instance_id,
            backend_handle,
            mods_state,
            mod_list,
        }
    }
}

impl Render for InstanceModsSubpage {
    fn render(&mut self, _window: &mut gpui::Window, cx: &mut gpui::Context<Self>) -> impl gpui::IntoElement {
        let theme = cx.theme();
        
        let state = self.mods_state.load(Ordering::SeqCst);
        if state.should_send_load_request() {
            self.backend_handle.blocking_send(MessageToBackend::RequestLoadMods { id: self.instance });
        }
        
        let header = h_flex()
            .gap_4()
            .mb_1()
            .ml_1()
            .child(div().text_lg().underline().child("Mods"))
            .child(Button::new("update").label("Check for updates").success().compact().small())
            .child(Button::new("addmr").label("Add from Modrinth").success().compact().small())
            .child(Button::new("addfile").label("Add from file").success().compact().small());
        
        v_flex()
            .p_4()
            .size_full()
            .child(header)
            .child(div().size_full().border_1().rounded(theme.radius).border_color(theme.border)
                .child(self.mod_list.clone()))
    }
}

pub struct ModsListDelegate {
    id: InstanceID,
    backend_handle: BackendHandle,
    mods: Vec<InstanceModSummary>,
    searched: Vec<InstanceModSummary>,
    confirming_delete: Arc<AtomicUsize>
}

impl ListDelegate for ModsListDelegate {
    type Item = ListItem;

    fn items_count(&self, _section: usize, _cx: &App) -> usize {
        self.searched.len()
    }

    fn render_item(
        &self,
        ix: IndexPath,
        _window: &mut Window,
        cx: &mut App,
    ) -> Option<Self::Item> {
        let summary = self.searched.get(ix.row)?;
        
        let icon = if let Some(png_icon) = summary.mod_summary.png_icon.as_ref() {
            png_render_cache::render(Arc::clone(png_icon), cx)
        } else {
            gpui::img(ImageSource::Resource(Resource::Embedded("images/default_mod.png".into())))
        };
        
        const GRAY: Hsla = Hsla { h: 0.0, s: 0.0, l: 0.5, a: 1.0};
        
        let description1 = v_flex()
            .w_1_5()
            .text_ellipsis()
            .child(SharedString::from(summary.mod_summary.name.clone()))
            .child(SharedString::from(summary.mod_summary.version_str.clone()));
        
        let description2 = v_flex()
            .text_color(GRAY)
            .child(SharedString::from(summary.mod_summary.authors.clone()))
            .child(SharedString::from(summary.filename.clone()));
        
        let id = self.id;
        let mod_id = summary.id;
        let element_id = summary.filename_hash;
        
        let delete_button = if self.confirming_delete.load(Ordering::Relaxed) == ix.row+1 {
            Button::new(("delete", element_id)).danger().icon(IconName::Check).on_click({
                let backend_handle = self.backend_handle.clone();
                move |_, _, _| {
                    backend_handle.blocking_send(MessageToBackend::DeleteMod {
                        id,
                        mod_id,
                    });
                }
            })
        } else {
            let trash_icon = Icon::default().path("icons/trash-2.svg");
            let confirming_delete = self.confirming_delete.clone();
            let delete_ix = ix.row+1;
            Button::new(("delete", element_id)).danger().icon(trash_icon).on_click(move |_, _, _| {
                confirming_delete.store(delete_ix, Ordering::Release);
            })
        };
        
        // let cant_update_icon = Icon::default().path("icons/arrow-down-to-dot.svg");
        // let update_icon = Icon::default().path("icons/arrow-down-to-line.svg");
        
        let backend_handle = self.backend_handle.clone();
        let item = ListItem::new(("item", element_id))
            .p_1()
            .child(h_flex()
                .gap_1()
                .child(Switch::new(("toggle", element_id)).checked(summary.enabled).on_click(move |checked, _, _| {
                    backend_handle.blocking_send(MessageToBackend::SetModEnabled {
                        id,
                        mod_id,
                        enabled: *checked
                    });
                }).px_2())
                .child(icon.size_16().min_w_16().min_h_16().grayscale(!summary.enabled))
                .when(!summary.enabled, |this| this.line_through())
                .child(description1)
                .child(description2)
                .child(delete_button.absolute().right_4())
            );
        
        Some(item)
    }
    
    fn set_selected_index(
        &mut self,
        _ix: Option<IndexPath>,
        _window: &mut Window,
        _cx: &mut Context<ListState<Self>>,
    ) {
    }
    
    fn perform_search(
        &mut self,
        query: &str,
        _window: &mut Window,
        _cx: &mut Context<ListState<Self>>,
    ) -> Task<()> {
        self.searched = self.mods.iter()
            .filter(|m| m.mod_summary.name.contains(query) || m.mod_summary.id.contains(query))
            .cloned()
            .collect();
        
        Task::ready(())
    }
}
