//! Field-typed clipboard adapter, shared by the menu and TRUEOS login prompt.
use std::cell::RefCell;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind {
    Text,
    Password,
}
#[derive(Default)]
pub struct Clipboard {
    #[cfg(target_os = "trueos")]
    window: Option<u32>,
    focused: Option<Kind>,
    message: RefCell<Option<String>>,
}
impl Clipboard {
    pub fn for_frame(window: u32) -> Self {
        #[cfg(target_os = "trueos")]
        {
            Self {
                window: Some(window),
                ..Self::default()
            }
        }
        #[cfg(not(target_os = "trueos"))]
        {
            let _ = window;
            Self::default()
        }
    }
    #[cfg(target_os = "trueos")]
    fn report(&self, text: &str) {
        *self.message.borrow_mut() = Some(text.to_owned());
    }
    pub fn take_message(&self) -> Option<String> {
        self.message.borrow_mut().take()
    }
    pub fn focus(&mut self, kind: Kind) {
        if self.focused == Some(kind) {
            return;
        }
        #[cfg(target_os = "trueos")]
        {
            let Some(window) = self.window else {
                self.report("TRUEOS clipboard is unavailable");
                return;
            };
            if let Err(e) = trueos::clipboard::focus(window, Some(os_kind(kind))) {
                self.report(e.message());
                return;
            }
        }
        self.focused = Some(kind);
    }
    pub fn blur(&mut self) {
        if self.focused.is_none() {
            return;
        }
        #[cfg(target_os = "trueos")]
        if let Some(window) = self.window {
            if let Err(e) = trueos::clipboard::focus(window, None) {
                self.report(e.message());
            }
        }
        self.focused = None;
    }
    pub fn copy(&mut self, kind: Kind, text: String) -> bool {
        #[cfg(target_os = "trueos")]
        {
            let Some(window) = self.window else {
                self.report("TRUEOS clipboard is unavailable");
                return false;
            };
            match trueos::clipboard::copy(window, os_kind(kind), &text) {
                Ok(()) => true,
                Err(e) => {
                    self.report(e.message());
                    false
                }
            }
        }
        #[cfg(not(target_os = "trueos"))]
        {
            conrod_core::clipboard::write_typed(text, kind == Kind::Password);
            true
        }
    }
    pub fn paste(&self, kind: Kind) -> Option<String> {
        #[cfg(target_os = "trueos")]
        {
            let Some(window) = self.window else {
                self.report("TRUEOS clipboard is unavailable");
                return None;
            };
            match trueos::clipboard::take_paste(window, os_kind(kind)) {
                Ok(text) => text,
                Err(e) => {
                    self.report(e.message());
                    None
                }
            }
        }
        #[cfg(not(target_os = "trueos"))]
        {
            conrod_core::clipboard::read_typed(kind == Kind::Password)
        }
    }
}
#[cfg(target_os = "trueos")]
fn os_kind(kind: Kind) -> trueos::clipboard::Kind {
    match kind {
        Kind::Text => trueos::clipboard::Kind::Text,
        Kind::Password => trueos::clipboard::Kind::Password,
    }
}
