//! Messages assembled locally from replayable content chunks.
use bas::model::{ContentBlock, Role, Timestamp};
use uuid::Uuid;

#[derive(Clone, Debug)]
pub(crate) struct Message {
    pub id: Uuid,
    pub role: Role,
    pub content: Vec<ContentBlock>,
    pub at: Timestamp,
}

impl Message {
    pub fn new(role: Role, content: Vec<ContentBlock>) -> Self {
        Self {
            id: Uuid::new_v4(),
            role,
            content,
            at: Timestamp::now(),
        }
    }

    pub fn plain_text(&self) -> String {
        self.content
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Text(text) => Some(text.text.as_str()),
                _ => None,
            })
            .collect()
    }
}
