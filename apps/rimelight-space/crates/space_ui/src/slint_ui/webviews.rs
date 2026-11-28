//! The native web views of the content editor's web-only blocks (CMS.6): rimelight_editor's `WebViewHost` over
//! rimelight_platform's `WebViews`.
//!
//! A native web view attaches to the window's native handle, which Slint exposes through its `raw-window-handle-06` feature
//! (proven in the CMS.0 spike on Windows). Android and the web place their views without it (rimelight_platform).
//!
//! While a shell overlay or a content dialog is open, every view is hidden, because a native child view draws over Slint.
//! The shell's timer calls [`WebHost::tick`] to show them again once it closes.

use std::collections::BTreeMap;

use rimelight_editor::{HostEvent, WebViewHost};
use rimelight_platform::webview::WebViews;
use slint::{ComponentHandle, Weak};

use crate::slint_ui::{AppWindow, ContentHomeState};

/// One web view's placement: the block key, the logical box, the window's scale and the URL it loads.
#[derive(Clone, Debug, PartialEq)]
pub struct Shot {
    pub key: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub scale: f32,
    pub url: String,
}

/// The app's web views: the window it draws over, the native views, and the last placement of each block.
pub struct WebHost {
    views: WebViews,
    window: Weak<AppWindow>,
    /// The last placement of each shown block, so its view comes back after a popup closes.
    placed: BTreeMap<String, Shot>,
    /// True while the views are hidden for a popup, until [`WebHost::tick`] shows them again.
    hidden: bool,
}

impl WebHost {
    pub fn new(window: Weak<AppWindow>) -> Self {
        Self {
            views: WebViews::new(),
            window,
            placed: BTreeMap::new(),
            hidden: false,
        }
    }

    /// True while a shell overlay (About, Feedback, Report, Settings) or the content New page dialog is open.
    fn popup_open(&self) -> bool {
        self.window.upgrade().is_some_and(|ui| {
            ui.get_overlay_open() || ui.global::<ContentHomeState>().get_new_open()
        })
    }

    /// Shows the views again when no popup is open any more. The shell calls it on a timer.
    pub fn tick(&mut self) {
        if !self.hidden || self.popup_open() {
            return;
        }
        let shots: Vec<Shot> = self.placed.values().cloned().collect();
        for shot in &shots {
            self.attach_shot(shot);
        }
        self.hidden = false;
    }

    /// Routes one editor host event. Only web-only blocks report a box; every other event is the page's.
    pub fn on_editor_event(&mut self, event: HostEvent) {
        if let HostEvent::WebGeometry {
            key,
            x,
            y,
            w,
            h,
            url,
        } = event
        {
            WebViewHost::show(self, &key, x, y, w, h, &url);
        }
    }

    fn attach_shot(&mut self, shot: &Shot) {
        let Some(ui) = self.window.upgrade() else {
            return;
        };
        let handle = ui.window().window_handle();
        if let Err(err) = self.views.show(
            &handle, &shot.key, shot.x, shot.y, shot.w, shot.h, shot.scale, &shot.url,
        ) {
            eprintln!("content web view {}: {err:?}", shot.key);
        }
    }
}

impl WebViewHost for WebHost {
    fn show(&mut self, key: &str, x: f32, y: f32, w: f32, h: f32, url: &str) {
        let scale = self
            .window
            .upgrade()
            .map_or(1.0, |ui| ui.window().scale_factor());
        let shot = Shot {
            key: key.to_string(),
            x,
            y,
            w,
            h,
            scale,
            url: url.to_string(),
        };
        self.placed.insert(key.to_string(), shot.clone());
        if self.popup_open() {
            self.hide_all();
            return;
        }
        self.attach_shot(&shot);
        self.hidden = false;
    }

    fn hide(&mut self, key: &str) {
        self.placed.remove(key);
        self.views.close(key);
    }

    fn hide_all(&mut self) {
        self.views.hide_all();
        self.hidden = true;
    }
}
