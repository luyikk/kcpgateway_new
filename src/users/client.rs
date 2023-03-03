use anyhow::{ensure, Result};
use bytes::BufMut;
use data_rw::DataOwnedReader;
use kcpserver::prelude::KCPPeer;
use std::fmt::{Display, Formatter};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::time::sleep;

use crate::get_len;
use crate::services::IServiceManager;
use crate::static_def::SERVICE_MANAGER;

/// 客户端client
pub struct Client {
    pub peer: KCPPeer,
    pub address: String,
    pub is_open_zero: AtomicBool,
}

impl Display for Client {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "({}-{})", self.peer.conv, self.peer.addr)
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        log::debug! {"Client:{} drop",self}
    }
}

impl Client {
    #[inline]
    pub fn new(peer: KCPPeer) -> Self {
        let address = peer.to_string();
        Self {
            peer,
            address,
            is_open_zero: Default::default(),
        }
    }
    /// 获取session id
    #[inline]
    pub fn get_session_id(&self) -> u32 {
        self.peer.conv
    }

    /// 立刻断线 同时清理
    #[inline]
    pub async fn disconnect_now(&self) {
        // 先关闭OPEN 0 标志位
        self.is_open_zero.store(false, Ordering::Release);

        // 管它有没有 每个服务器都调用下 DropClientPeer 让服务器的 DropClientPeer 自己检查
        SERVICE_MANAGER
            .disconnect_events(self.get_session_id())
            .await;

        // 断线
        let _ = self.peer.get_writer().shutdown().await;
    }

    /// 服务器open ok
    #[inline]
    pub async fn open_service(&self, service_id: u32) -> Result<()> {
        log::info!(
            "service:{} open peer:{} OK",
            service_id,
            self.get_session_id()
        );
        self.is_open_zero.store(true, Ordering::Release);
        self.send_open(service_id).await
    }

    /// 服务器通知 关闭某个服务
    #[inline]
    pub async fn close_service(&self, service_id: u32) -> Result<()> {
        log::info!(
            "service:{} close peer:{} ok",
            service_id,
            self.get_session_id()
        );
        if service_id == 0 {
            self.kick().await
        } else {
            self.send_close(service_id).await
        }
    }

    /// kick 命令
    #[inline]
    pub async fn kick_by_delay(&self, service_id: u32, mut delay_ms: i32) -> Result<()> {
        if !(0..=30000).contains(&delay_ms) {
            delay_ms = 5000;
        }
        log::info!(
            "service:{} delay kick peer:{} delay_ms:{}",
            service_id,
            self,
            delay_ms
        );
        self.send_close(0).await?;
        let peer = self.peer.clone();
        tokio::spawn(async move {
            sleep(Duration::from_millis(delay_ms as u64)).await;
            log::info!("start kick peer:{}", peer.conv);
            if let Err(err) = peer.get_writer().shutdown().await {
                log::warn!("kick {} send disconnect err:{}", peer.conv, err);
            }
        });
        Ok(())
    }

    /// 发送 CLOSE 0 后立即断线清理内存
    #[inline]
    async fn kick(&self) -> Result<()> {
        log::info!("start kick peer:{} now", self.get_session_id());
        self.send_close(0).await?;
        self.disconnect_now().await;
        Ok(())
    }

    /// 发送服务器open
    #[inline]
    async fn send_open(&self, service_id: u32) -> Result<()> {
        let mut buffer = data_rw::Data::new();
        buffer.write_fixed(0u32);
        buffer.write_fixed(0xFFFFFFFFu32);
        buffer.write_var_integer("open");
        buffer.write_var_integer(service_id);
        let len = get_len!(buffer);
        (&mut buffer[0..4]).put_u32_le(len);
        self.peer.send(&buffer).await?;
        Ok(())
    }

    /// 发送close 命令
    #[inline]
    async fn send_close(&self, service_id: u32) -> Result<()> {
        let mut buffer = data_rw::Data::new();
        buffer.write_fixed(0u32);
        buffer.write_fixed(0xFFFFFFFFu32);
        buffer.write_var_integer("close");
        buffer.write_var_integer(service_id);
        let len = get_len!(buffer);
        (&mut buffer[0..4]).put_u32_le(len);
        self.peer.send(&buffer).await?;
        Ok(())
    }

    /// 发送数据包给客户端
    #[inline]
    pub async fn send(&self, session_id: u32, buff: &[u8]) -> Result<()> {
        let mut buffer = data_rw::Data::new();
        buffer.write_fixed(0u32);
        buffer.write_fixed(session_id);
        buffer.write_buf(buff);
        let len = get_len!(buffer);
        (&mut buffer[0..4]).put_u32_le(len);
        self.peer.send(&buffer).await?;
        Ok(())
    }
}

/// 客户端数据包处理
#[inline]
pub async fn input_buff(client: &Arc<Client>, data: Vec<u8>) -> Result<()> {
    ensure!(data.len() > 4, "peer:{} data len:{} <4", client, data.len());
    let mut reader = DataOwnedReader::new(data);
    let server_id = reader.read_fixed::<u32>()?;
    if u32::MAX == server_id {
        //给网关发送数据包,默认当PING包无脑回
        if let Err(err) = client.send(server_id, &reader[reader.get_offset()..]).await {
            log::error!("peer:{} send ping error:{}", client, err);
        }
        Ok(())
    } else {
        SERVICE_MANAGER
            .send_buffer(client.get_session_id(), server_id, reader)
            .await
    }
}
