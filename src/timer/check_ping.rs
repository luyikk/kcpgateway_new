use crate::services::IServiceManager;
use crate::static_def::SERVICE_MANAGER;
use crate::timer::Timer;

pub struct PingCheckTimer;

#[async_trait::async_trait]
impl Timer for PingCheckTimer {
    async fn init(&self) -> anyhow::Result<(bool, u64)> {
        Ok((false, 5000))
    }

    async fn run(&self) -> anyhow::Result<()> {
        SERVICE_MANAGER.check_ping().await
    }
}
