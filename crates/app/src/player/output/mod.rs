mod playout;

use crate::{player::controller::ChannelManager, utils::errors::ServiceError};

/// Player entry point backed by crates/engine.
pub async fn player(manager: ChannelManager) -> Result<(), ServiceError> {
    playout::player(manager).await
}
