use crate::services::IServiceManager;
use anyhow::ensure;
use kcpserver::prelude::kcp_module::{KcpConfig, KcpNoDelayConfig};
use kcpserver::prelude::{KCPPeer, KcpListener, KcpReader};
use std::error::Error;
use std::future::Future;
use std::net::ToSocketAddrs;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncReadExt;

use crate::static_def::{SERVICE_MANAGER, USER_MANAGER};
use crate::users::{input_buff, Client, IUserManager};

/// 最大数据表长度限制 512K
const MAX_BUFF_LEN: usize = 512 * 1024;

/// 客户端监听服务
pub struct Listen {
    server: Box<dyn IKcpServer>,
}

impl Listen {
    /// 创建监听
    pub fn new<T: ToSocketAddrs>(address: T, drop_timeout_sec: u64) -> anyhow::Result<Self> {
        let config = KcpConfig {
            nodelay: Some(KcpNoDelayConfig::fastest()),
            ..Default::default()
        };
        let server = KcpListener::new(address, config, drop_timeout_sec, Self::input)?;
        Ok(Self {
            server: Box::new(server),
        })
    }

    /// 启动监听
    pub async fn start(&self) -> anyhow::Result<()> {
        self.server.start().await
    }

    /// 客户端连接
    #[inline]
    async fn input(peer: KCPPeer) -> Result<(), Box<dyn Error>> {
        let mut reader = peer.get_reader();
        if reader.read_u32_le().await? == 1 && reader.read_u8().await? == 0 {
            let client = USER_MANAGER.make_client(peer.clone()).await?;
            let res = Self::data_input(reader, client).await;
            // 等2秒,防止通知大厅disconnect的时候 peer还没有创建
            tokio::time::sleep(Duration::from_secs(2)).await;
            USER_MANAGER.remove_client(peer.conv).await;
            res?;
            Ok(())
        } else {
            log::debug!("kcp peer:{} read 1 0 0 0 0 error", peer);
            Ok(())
        }
    }

    /// 数据包处理
    #[inline]
    async fn data_input(mut reader: KcpReader<'_>, client: Arc<Client>) -> anyhow::Result<()> {
        log::debug!("create peer:{}", client);
        SERVICE_MANAGER
            .open_service(client.get_session_id(), 0, &client.address)
            .await?;
        loop {
            let len = {
                match reader.read_u32_le().await {
                    Ok(len) => len as usize,
                    Err(err) => {
                        log::warn!("peer:{} disconnect,err:{}", client, err);
                        break;
                    }
                }
            };
            //如果没有OPEN 直接掐线
            if !client.is_open_zero.load(Ordering::Acquire) {
                log::warn!("peer:{} not open send data,disconnect!", client);
                break;
            }

            // 如果长度为0 或者超过最大限制 掐线
            if len >= MAX_BUFF_LEN || len <= 4 {
                log::warn!("disconnect peer:{} packer len error:{}", client, len);
                break;
            }

            let mut data = vec![0; len];
            match reader.read_exact(&mut data).await {
                Ok(rev) => {
                    ensure!(
                        len == rev,
                        "peer:{} read buff error len:{}>rev:{}",
                        client,
                        len,
                        rev
                    );

                    input_buff(&client, data).await?;
                }
                Err(err) => {
                    log::error!("peer:{} read data error:{:?}", client, err);
                    break;
                }
            }
        }

        Ok(())
    }
}

/// kcp server 化泛型
#[async_trait::async_trait]
pub trait IKcpServer {
    async fn start(&self) -> anyhow::Result<()>;
}

#[async_trait::async_trait]
impl<I, R> IKcpServer for Arc<KcpListener<I>>
where
    I: Fn(KCPPeer) -> R + Send + Sync + 'static,
    R: Future<Output = Result<(), Box<dyn Error>>> + Send + 'static,
{
    async fn start(&self) -> anyhow::Result<()> {
        self.start().await?;
        Ok(())
    }
}
