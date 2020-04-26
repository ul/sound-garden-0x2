use audio_program::TextOp;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub enum Message {
    Play(bool),
    Record(bool),
    LoadProgram(Vec<TextOp>),
}
