//! Plain-text clipboard shared within the process, without OS integration.
//! Password-tagged contents are available only through the typed interface.
use std::sync::Mutex;
struct Contents {
    text: String,
    secure: bool,
}
static CONTENTS: Mutex<Contents> = Mutex::new(Contents {
    text: String::new(),
    secure: false,
});

/// Read ordinary text. Password contents are never exposed to ordinary widgets.
pub fn read() -> String {
    read_typed(false).unwrap_or_default()
}
/// Replace the clipboard with ordinary text.
pub fn write(text: String) {
    write_typed(text, false);
}
/// Read only contents classified for the requesting field.
pub fn read_typed(secure: bool) -> Option<String> {
    let contents = CONTENTS.lock().unwrap_or_else(|error| error.into_inner());
    if contents.secure == secure {
        Some(contents.text.clone())
    } else {
        None
    }
}
/// Share the local clipboard while preserving password classification.
pub fn write_typed(text: String, secure: bool) {
    *CONTENTS.lock().unwrap_or_else(|error| error.into_inner()) = Contents { text, secure };
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn password_never_appears_in_plain_clipboard() {
        write_typed("secret".into(), true);
        assert_eq!(read(), "");
        assert_eq!(read_typed(true).as_deref(), Some("secret"));
        write("ordinary".into());
        assert_eq!(read(), "ordinary");
        assert_eq!(read_typed(true), None);
    }
}
