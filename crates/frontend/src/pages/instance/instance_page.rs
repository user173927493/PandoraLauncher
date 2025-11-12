use bridge::{handle::BackendHandle, instance::{InstanceID, InstanceStatus}, message::MessageToBackend};
use gpui::{prelude::*, *};
use gpui_component::{
    button::{Button, ButtonVariants}, h_flex, tab::{Tab, TabBar}, Icon, IconName
};

use crate::{entity::{instance::InstanceEntry, DataEntities}, pages::instance::{mods_subpage::InstanceModsSubpage, quickplay_subpage::InstanceQuickplaySubpage}, root, ui};

pub struct InstancePage {
    backend_handle: BackendHandle,
    pub title: SharedString,
    instance: Entity<InstanceEntry>,
    subpage: InstanceSubpage,
    _instance_subscription: Subscription,
}

enum InstanceSubpageType {
    Quickplay,
    Mods
}

impl InstancePage {
    pub fn new(instance_id: InstanceID, data: &DataEntities, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let instance = data.instances.read(cx).entries.get(&instance_id).unwrap().clone();
        
        let _instance_subscription = cx.observe(&instance, |page, instance, cx| {
            let instance = instance.read(cx);
            page.title = instance.title().into();
        });
        
        let subpage = InstanceSubpage::Quickplay(cx.new(|cx| {
            InstanceQuickplaySubpage::new(&instance, data.backend_handle.clone(), window, cx)
        }));
        
        Self {
            backend_handle: data.backend_handle.clone(),
            title: instance.read(cx).title().into(),
            instance,
            subpage,
            _instance_subscription
        }
    }
    
    fn set_subpage(&mut self, page_type: InstanceSubpageType, window: &mut Window, cx: &mut Context<Self>) {
        match page_type {
            InstanceSubpageType::Quickplay => {
                if let InstanceSubpage::Quickplay(_) = self.subpage {
                    return;
                }
                
                self.subpage = InstanceSubpage::Quickplay(cx.new(|cx| {
                    InstanceQuickplaySubpage::new(&self.instance, self.backend_handle.clone(), window, cx)
                }));
            },
            InstanceSubpageType::Mods => {
                if let InstanceSubpage::Mods(_) = self.subpage {
                    return;
                }
                
                self.subpage = InstanceSubpage::Mods(cx.new(|cx| {
                    InstanceModsSubpage::new(&self.instance, self.backend_handle.clone(), window, cx)
                }));
            },
        }
    }
}

impl Render for InstancePage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let selected_index = match &self.subpage {
            InstanceSubpage::Quickplay(_) => 0,
            InstanceSubpage::Mods(_) => 2,
        };
        
        let play_icon = Icon::empty().path("icons/play.svg");
        
        let instance = self.instance.read(cx);
        let id = instance.id;
        let name = instance.name.clone();
        let backend_handle = self.backend_handle.clone();
        
        let button = match instance.status {
            InstanceStatus::NotRunning => {
                Button::new("start_instance")
                    .success()
                    .icon(play_icon)
                    .label("Start Instance")
                    .on_click(move |_, window, cx| {
                        root::start_instance(id, name.clone(), None, &backend_handle, window, cx);
                    })
            },
            InstanceStatus::Launching => {
                Button::new("launching")
                    .warning()
                    .icon(IconName::Loader)
                    .label("Launching...")
            },
            InstanceStatus::Running => {
                Button::new("kill_instance")
                    .danger()
                    .icon(IconName::Close)
                    .label("Kill Instance")
                    .on_click(move |_, _, _| {
                        backend_handle.blocking_send(MessageToBackend::KillInstance { id });
                    })
            },
        };
        
        ui::page(cx, h_flex().gap_8().child(self.title.clone()).child(button))
            .child(TabBar::new("bar").prefix(div().w_4()).selected_index(selected_index).underline().child(Tab::new("Quickplay")).child(Tab::new("Logs")).child(Tab::new("Mods")).child(Tab::new("Resource Packs")).child(Tab::new("Worlds"))
                .on_click(cx.listener(|page, index, window, cx| {
                    let page_type = match *index {
                        0 => InstanceSubpageType::Quickplay,
                        2 => InstanceSubpageType::Mods,
                        _ => {
                            return;
                        }
                    };
                    page.set_subpage(page_type, window, cx);
                })))
            .child(self.subpage.clone().into_any_element())
            
    }
}

#[derive(Clone)]
pub enum InstanceSubpage {
    Quickplay(Entity<InstanceQuickplaySubpage>),
    Mods(Entity<InstanceModsSubpage>)
}

impl InstanceSubpage {
    pub fn into_any_element(self) -> AnyElement {
        match self {
            Self::Quickplay(entity) => entity.into_any_element(),
            Self::Mods(entity) => entity.into_any_element(),
        }
    }
}
