use serde::{Deserialize, Serialize};

/// A message sent to the panel giving all of the information it needs to
/// populate the display.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DisplayMessage {
    /// The "person is:" message.
    pub person_is: String,
}

impl Default for DisplayMessage {
    fn default() -> Self {
        DisplayMessage {
            person_is: "whereabouts unknown".to_owned(),
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
