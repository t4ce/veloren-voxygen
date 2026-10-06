//! Access the clipboard.

/// A buffer for short-term storage and transfer within and between
/// applications.
pub trait Clipboard {
    /// Reads the current content of the [`Clipboard`] as text.
    fn read(&self) -> Option<String>;

    /// Writes the given text contents to the [`Clipboard`].
    fn write(&mut self, contents: String);

    /// Declare the focused field's delivery type before a trusted paste.
    fn focus(&mut self, _secure: bool) {}
    /// Typed reads must never downgrade a password to ordinary text.
    fn read_typed(&self, secure: bool) -> Option<String> { if secure { None } else { self.read() } }
    /// Return false when copying was rejected, so cut retains the selection.
    fn write_typed(&mut self, contents: String, secure: bool) -> bool { if secure { false } else { self.write(contents); true } }
}

/// A null implementation of the [`Clipboard`] trait.
#[derive(Debug, Clone, Copy)]
pub struct Null;

impl Clipboard for Null {
    fn read(&self) -> Option<String> {
        None
    }

    fn write(&mut self, _contents: String) {}
}
