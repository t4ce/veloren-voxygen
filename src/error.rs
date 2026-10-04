use crate::render::RenderError;
use core::fmt::Debug;

/// Represents any error that may be triggered by Voxygen.
#[derive(Debug)]
pub enum Error {
    /// An error relating to the internal client.
    ClientError(crate::client::Error),
    /// A miscellaneous error relating to a backend dependency.
    BackendError(Box<dyn Debug>),
    /// An error relating the rendering subsystem.
    RenderError(RenderError),
}

impl From<RenderError> for Error {
    fn from(err: RenderError) -> Self { Error::RenderError(err) }
}

impl From<crate::client::Error> for Error {
    fn from(err: crate::client::Error) -> Self { Error::ClientError(err) }
}
