use serde::{Deserialize, Serialize};

/// A message sent to the panel giving all of the information it needs to
/// populate the display.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DisplayMessage {
    /// Some message.
    pub message: String,
}

/// A message sent to hub from a display client introducing itself.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct HelloMessage {
    /// Some number.
    pub a_number: u32,
}
