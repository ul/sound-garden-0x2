#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("{0}")]
    TextureCopy(String),
    #[error("{0}")]
    SDLInit(String),
    #[error("{0}")]
    Video(String),
    #[error("{0}")]
    EventPump(String),
    #[error("{0}")]
    LoadFont(String),
    #[error("{0}")]
    Draw(String),
}
