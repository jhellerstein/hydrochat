//! Shared types and protocol for HydroChat

pub mod protocol;

#[derive(Clone, Debug)]
pub struct ChatClient {
    nickname: String,
    connected: bool,
}

impl ChatClient {
    pub fn new(nickname: String) -> Self {
        Self {
            nickname,
            connected: false,
        }
    }

    pub fn get_nickname(&self) -> String {
        self.nickname.clone()
    }

    pub fn is_connected(&self) -> bool {
        self.connected
    }

    pub fn set_connected(&mut self, connected: bool) {
        self.connected = connected;
    }
}
