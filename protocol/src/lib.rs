use serde::{Deserialize, Serialize};

pub type Timestamp = chrono::DateTime<chrono::Utc>;

/// A message sent to the panel giving all of the information it needs to
/// populate the display.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DisplayMessage {
    /// The "person is:" message.
    pub person_is: String,

    /// When the "person is:" message was last updated.
    pub person_is_timestamp: Timestamp,
}

impl Default for DisplayMessage {
    fn default() -> Self {
        DisplayMessage {
            person_is: "whereabouts unknown".to_owned(),
            person_is_timestamp: chrono::Utc::now(),
        }
    }
}

/// A "hello" from a displayer client.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DisplayHelloMessage {}

/// A "hello" from a "person is"-update client.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PersonIsUpdateHelloMessage {
    /// The new "person is:" message.
    pub person_is: String,

    /// The message timestamp.
    pub timestamp: Timestamp,
}

/// A message sent to hub from a client introducing itself.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum ClientHelloMessage {
    /// This client wants to subscribe to display updates, and will presumably
    /// display them on the stickynote device.
    Display(DisplayHelloMessage),

    /// This client wants to update the "person is:" message.
    PersonIsUpdate(PersonIsUpdateHelloMessage),
}

/// Validate a "person_is" message.
///
/// We just check length against an empirical limit based on the current
/// display size and font setup. The font used is variable-width so there's
/// some slop but we don't need to be exactly perfect.
pub fn is_person_is_valid(person_is: &str) -> bool {
    person_is.len() < 23
}
