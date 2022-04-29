use std::io;
use std::sync::Arc;

use async_trait::async_trait;
use log::*;
use tokio::sync::RwLock;

use crate::{app::outbound::selector::OutboundSelector, proxy::*, session::Session};

pub struct Handler {
    pub selector: Arc<RwLock<OutboundSelector>>,
}

#[async_trait]
impl UdpOutboundHandler for Handler {
    fn connect_addr(&self) -> Option<OutboundConnect> {
        None
    }

    fn transport_type(&self) -> DatagramTransportType {
        DatagramTransportType::Undefined
    }

    async fn handle<'a>(
        &'a self,
        sess: &'a Session,
        transport: Option<AnyOutboundTransport>,
    ) -> io::Result<AnyOutboundDatagram> {
        if let Some(a) = self.selector.read().await.get_selected() {
            debug!("select handles tcp [{}] to [{}]", sess.destination, a.tag());
            UdpOutboundHandler::handle(a.as_ref(), sess, transport).await
        } else {
            Err(io::Error::new(io::ErrorKind::Other, "no selected outbound"))
        }
    }
}
