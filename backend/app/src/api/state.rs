use std::sync::Arc;

use sqlx::sqlite::SqlitePool;
use tokio::sync::{Mutex, RwLock};

use crate::{
    api::file_access::FileAccessState,
    player::controller::ChannelController,
    sse::{SseAuthState, broadcast::Broadcaster},
    utils::{mail::MailQueue, system::SystemStat},
};

#[derive(Clone)]
pub struct AppState {
    pub auth_state: Arc<SseAuthState>,
    pub broadcaster: Arc<Broadcaster>,
    pub controller: Arc<RwLock<ChannelController>>,
    pub file_access: Arc<FileAccessState>,
    pub mail_queues: Arc<Mutex<Vec<Arc<Mutex<MailQueue>>>>>,
    pub pool: SqlitePool,
    pub system: SystemStat,
}
