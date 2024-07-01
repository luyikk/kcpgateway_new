pub mod client;
pub mod listen;
pub use client::*;
pub use listen::*;

use ahash::AHashMap;
use anyhow::{ensure, Result};
use aqueue::Actor;
use data_rw::DataOwnedReader;
use kcpserver::prelude::KCPPeer;
use std::sync::Arc;

/// 用户管理类
pub struct UserManager {
    /// 用户表
    users: AHashMap<u32, Arc<Client>>,
}

impl UserManager {
    pub fn new() -> Actor<Self> {
        Actor::new(Self {
            users: Default::default(),
        })
    }

    ///制造一个client
    #[inline]
    fn make_client(&mut self, peer: KCPPeer) -> Result<Arc<Client>> {
        let client = Arc::new(Client::new(peer));
        ensure!(
            self.users
                .insert(client.get_session_id(), client.clone())
                .is_none(),
            "session id:{} repeat",
            client.get_session_id()
        );
        Ok(client)
    }

    /// 删除一个client
    #[inline]
    async fn remove_client(&mut self, session_id: u32) {
        if let Some(peer) = self.users.remove(&session_id) {
            peer.disconnect_now().await;
        }
    }

    /// send open peer 通知
    #[inline]
    async fn open_service(&self, service_id: u32, session_id: u32) -> Result<()> {
        if let Some(client) = self.users.get(&session_id) {
            if let Err(err) = client.open_service(service_id).await {
                log::error!(
                    "service:{} peer:{} open_service error:{}",
                    service_id,
                    session_id,
                    err
                );
            }
        }
        Ok(())
    }

    /// close service 通知
    #[inline]
    async fn close_service(&self, service_id: u32, session_id: u32) -> Result<()> {
        if let Some(client) = self.users.get(&session_id) {
            if let Err(err) = client.close_service(service_id).await {
                log::error!(
                    "service:{} peer:{} close_service error:{}",
                    service_id,
                    session_id,
                    err
                );
            }
        }
        Ok(())
    }

    /// kick client 通知
    #[inline]
    async fn kick_client(&self, service_id: u32, session_id: u32, delay_ms: i32) -> Result<()> {
        if let Some(client) = self.users.get(&session_id) {
            if let Err(err) = client.kick_by_delay(service_id, delay_ms).await {
                log::error!(
                    "service:{} peer:{} delay_ms:{} kick_by_delay error:{}",
                    service_id,
                    session_id,
                    delay_ms,
                    err
                );
            }
        }
        Ok(())
    }

    /// 发送buff
    #[inline]
    async fn send_buffer(
        &self,
        service_id: u32,
        session_id: u32,
        buff: DataOwnedReader,
    ) -> Result<()> {
        if let Some(client) = self.users.get(&session_id) {
            if let Err(err) = client.send(service_id, &buff[buff.get_offset()..]).await {
                log::error!(
                    "service:{}  peer:{} send buffer error:{:?}",
                    service_id,
                    session_id,
                    err
                );
            }
        }
        Ok(())
    }
}

pub trait IUserManager {
    /// 制造一个 client
    async fn make_client(&self, peer: KCPPeer) -> Result<Arc<Client>>;
    /// 删除一个client
    async fn remove_client(&self, session_id: u32);
    /// send open service 通知
    async fn open_service(&self, service_id: u32, session_id: u32) -> Result<()>;
    /// close service 通知
    async fn close_service(&self, service_id: u32, session_id: u32) -> Result<()>;
    /// kick client 通知
    async fn kick_client(&self, service_id: u32, session_id: u32, delay_ms: i32) -> Result<()>;
    /// 发送buff
    async fn send_buffer(
        &self,
        service_id: u32,
        session_id: u32,
        buff: DataOwnedReader,
    ) -> Result<()>;
}

impl IUserManager for Actor<UserManager> {
    #[inline]
    async fn make_client(&self, peer: KCPPeer) -> Result<Arc<Client>> {
        self.inner_call(|inner| async move { inner.get_mut().make_client(peer) })
            .await
    }
    #[inline]
    async fn remove_client(&self, session_id: u32) {
        self.inner_call(|inner| async move { inner.get_mut().remove_client(session_id).await })
            .await
    }

    #[inline]
    async fn open_service(&self, service_id: u32, session_id: u32) -> Result<()> {
        self.inner_call(
            |inner| async move { inner.get().open_service(service_id, session_id).await },
        )
        .await
    }

    #[inline]
    async fn close_service(&self, service_id: u32, session_id: u32) -> Result<()> {
        self.inner_call(
            |inner| async move { inner.get().close_service(service_id, session_id).await },
        )
        .await
    }

    #[inline]
    async fn kick_client(&self, service_id: u32, session_id: u32, delay_ms: i32) -> Result<()> {
        self.inner_call(|inner| async move {
            inner
                .get()
                .kick_client(service_id, session_id, delay_ms)
                .await
        })
        .await
    }
    #[inline]
    async fn send_buffer(
        &self,
        service_id: u32,
        session_id: u32,
        buff: DataOwnedReader,
    ) -> Result<()> {
        self.inner_call(|inner| async move {
            inner.get().send_buffer(service_id, session_id, buff).await
        })
        .await
    }
}
