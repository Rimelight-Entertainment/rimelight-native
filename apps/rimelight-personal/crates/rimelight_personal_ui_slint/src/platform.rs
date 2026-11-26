//! The desktop platform services every page shares: files dropped onto the window, the clipboard, and opening a URL or
//! a file in the system.
//!
//! Pages never call the OS themselves. They take a [`Platform`] (a trait, so a page's test injects [`fake::FakePlatform`]
//! and reads back the calls) and subscribe to the [`FileDrops`] registry while they are shown.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::{Rc, Weak};

use slint::Window;
use slint::winit_030::winit::event::WindowEvent;
use slint::winit_030::{EventResult, WinitWindowAccessor};

/// A file dragged over the window, as winit reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileDrop {
    /// A file is being dragged over the window.
    Hovered(PathBuf),
    /// The drag left the window without a drop.
    HoveredCancelled,
    /// A file was dropped on the window.
    Dropped(PathBuf),
}

impl FileDrop {
    /// The drop event a winit window event means, if it is one of the three file-drop events.
    pub fn from_winit(event: &WindowEvent) -> Option<Self> {
        match event {
            WindowEvent::HoveredFile(path) => Some(Self::Hovered(path.clone())),
            WindowEvent::HoveredFileCancelled => Some(Self::HoveredCancelled),
            WindowEvent::DroppedFile(path) => Some(Self::Dropped(path.clone())),
            _ => None,
        }
    }
}

type Handler = Rc<dyn Fn(&FileDrop)>;

#[derive(Default)]
struct Registry {
    next_id: u64,
    subscribers: Vec<(u64, Handler)>,
    hovering: bool,
}

/// The window's file-drop registry. Clones share one registry. A page subscribes while it is shown and drops the
/// [`Subscription`] when it is not.
#[derive(Clone, Default)]
pub struct FileDrops {
    inner: Rc<RefCell<Registry>>,
}

/// A live subscription. Dropping it unsubscribes.
pub struct Subscription {
    registry: Weak<RefCell<Registry>>,
    id: u64,
}

impl Drop for Subscription {
    fn drop(&mut self) {
        if let Some(registry) = self.registry.upgrade() {
            registry
                .borrow_mut()
                .subscribers
                .retain(|(id, _)| *id != self.id);
        }
    }
}

impl FileDrops {
    /// Hooks the window's winit events, so the OS file drops reach this registry. Each event is passed on afterwards.
    pub fn install(window: &Window) -> Self {
        let drops = Self::default();
        let hook = drops.clone();
        window.on_winit_window_event(move |_, event| {
            if let Some(drop) = FileDrop::from_winit(event) {
                hook.dispatch(&drop);
            }
            EventResult::Propagate
        });
        drops
    }

    /// Calls `handler` for every file-drop event until the returned [`Subscription`] is dropped.
    pub fn subscribe(&self, handler: impl Fn(&FileDrop) + 'static) -> Subscription {
        let mut registry = self.inner.borrow_mut();
        registry.next_id += 1;
        let id = registry.next_id;
        registry.subscribers.push((id, Rc::new(handler)));
        Subscription {
            registry: Rc::downgrade(&self.inner),
            id,
        }
    }

    /// Whether a file is being dragged over the window right now: the bool the drop zones highlight on.
    pub fn hovering(&self) -> bool {
        self.inner.borrow().hovering
    }

    /// Records `drop` and calls every subscriber. The handlers are copied out first, so a handler may subscribe or drop
    /// a subscription without a borrow conflict.
    pub fn dispatch(&self, drop: &FileDrop) {
        let handlers: Vec<Handler> = {
            let mut registry = self.inner.borrow_mut();
            registry.hovering = matches!(drop, FileDrop::Hovered(_));
            registry
                .subscribers
                .iter()
                .map(|(_, handler)| handler.clone())
                .collect()
        };
        for handler in handlers {
            handler(drop);
        }
    }
}

/// The platform services a page uses. [`System`] calls the OS; tests inject [`fake::FakePlatform`].
pub trait Platform {
    /// Puts `text` on the clipboard.
    fn copy_text(&self, text: &str) -> Result<(), String>;
    /// Opens `target` (a URL or a file path) in the system's handler for it.
    fn open_url(&self, target: &str) -> Result<(), String>;
}

/// The real platform: the clipboard through arboard, opening through the `open` crate.
pub struct System;

impl Platform for System {
    fn copy_text(&self, text: &str) -> Result<(), String> {
        copy_text(text)
    }

    fn open_url(&self, target: &str) -> Result<(), String> {
        open_url(target)
    }
}

/// Puts `text` on the system clipboard.
pub fn copy_text(text: &str) -> Result<(), String> {
    arboard::Clipboard::new()
        .and_then(|mut clipboard| clipboard.set_text(text))
        .map_err(|e| e.to_string())
}

/// Opens `target` (a URL or a file path) with the system's default handler.
pub fn open_url(target: &str) -> Result<(), String> {
    open::that(target).map_err(|e| e.to_string())
}

#[cfg(test)]
pub(crate) mod fake {
    use std::cell::RefCell;

    use super::Platform;

    /// Answers every call at once and records it. The page tests read `copies` and `opens` back.
    #[derive(Default)]
    pub struct FakePlatform {
        pub copies: RefCell<Vec<String>>,
        pub opens: RefCell<Vec<String>>,
        /// When set, every call fails with this message.
        pub fail_with: RefCell<Option<String>>,
    }

    impl Platform for FakePlatform {
        fn copy_text(&self, text: &str) -> Result<(), String> {
            self.copies.borrow_mut().push(text.to_owned());
            self.outcome()
        }

        fn open_url(&self, target: &str) -> Result<(), String> {
            self.opens.borrow_mut().push(target.to_owned());
            self.outcome()
        }
    }

    impl FakePlatform {
        fn outcome(&self) -> Result<(), String> {
            match self.fail_with.borrow().as_ref() {
                Some(message) => Err(message.clone()),
                None => Ok(()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::path::PathBuf;
    use std::rc::Rc;

    use slint::winit_030::winit::event::WindowEvent;

    use super::*;

    #[test]
    fn winit_file_events_map_to_drops_and_others_are_ignored() {
        let path = PathBuf::from("take.mp3");
        assert_eq!(
            FileDrop::from_winit(&WindowEvent::HoveredFile(path.clone())),
            Some(FileDrop::Hovered(path.clone()))
        );
        assert_eq!(
            FileDrop::from_winit(&WindowEvent::HoveredFileCancelled),
            Some(FileDrop::HoveredCancelled)
        );
        assert_eq!(
            FileDrop::from_winit(&WindowEvent::DroppedFile(path.clone())),
            Some(FileDrop::Dropped(path))
        );
        assert_eq!(FileDrop::from_winit(&WindowEvent::Focused(true)), None);
    }

    #[test]
    fn the_hovering_flag_follows_the_drag_and_the_drop_ends_it() {
        let drops = FileDrops::default();
        assert!(!drops.hovering());
        drops.dispatch(&FileDrop::Hovered(PathBuf::from("a.pdf")));
        assert!(drops.hovering());
        drops.dispatch(&FileDrop::HoveredCancelled);
        assert!(!drops.hovering());
        drops.dispatch(&FileDrop::Hovered(PathBuf::from("a.pdf")));
        drops.dispatch(&FileDrop::Dropped(PathBuf::from("a.pdf")));
        assert!(!drops.hovering());
    }

    #[test]
    fn a_subscriber_gets_events_until_its_subscription_is_dropped() {
        let drops = FileDrops::default();
        let seen: Rc<RefCell<Vec<FileDrop>>> = Rc::default();
        let log = seen.clone();
        let subscription = drops.subscribe(move |drop| log.borrow_mut().push(drop.clone()));

        drops.dispatch(&FileDrop::Dropped(PathBuf::from("a.mp3")));
        drop(subscription);
        drops.dispatch(&FileDrop::Dropped(PathBuf::from("b.mp3")));

        assert_eq!(*seen.borrow(), [FileDrop::Dropped(PathBuf::from("a.mp3"))]);
    }

    #[test]
    fn a_handler_may_drop_its_own_subscription_during_dispatch() {
        let drops = FileDrops::default();
        let slot: Rc<RefCell<Option<Subscription>>> = Rc::default();
        let holder = slot.clone();
        let subscription = drops.subscribe(move |_| {
            holder.borrow_mut().take();
        });
        *slot.borrow_mut() = Some(subscription);
        drops.dispatch(&FileDrop::HoveredCancelled);
        assert!(slot.borrow().is_none());
    }
}
