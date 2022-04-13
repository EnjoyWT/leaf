use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;

use warp::Filter;

use crate::RuntimeManager;

mod models {
    use serde_derive::{Deserialize, Serialize};

    #[derive(Debug, Deserialize)]
    pub struct SelectOptions {
        pub outbound: Option<String>,
        pub select: Option<String>,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct SelectReply {
        pub selected: Option<String>,
    }
}

mod handlers {
    use super::*;
    use warp::http::StatusCode;

    pub async fn select_update(
        opts: models::SelectOptions,
        rm: Arc<RuntimeManager>,
    ) -> Result<impl warp::Reply, Infallible> {
        if let models::SelectOptions {
            outbound: Some(outbound),
            select: Some(select),
        } = opts
        {
            if rm.set_outbound_selected(&outbound, &select).await.is_ok() {
                return Ok(StatusCode::OK);
            }
        }
        Ok(StatusCode::ACCEPTED)
    }

    pub async fn select_get(
        opts: models::SelectOptions,
        rm: Arc<RuntimeManager>,
    ) -> Result<impl warp::Reply, Infallible> {
        if let models::SelectOptions {
            outbound: Some(outbound),
            ..
        } = opts
        {
            if let Ok(selected) = rm.get_outbound_selected(&outbound).await {
                return Ok(warp::reply::json(&models::SelectReply {
                    selected: Some(selected),
                }));
            }
        }
        Ok(warp::reply::json(&models::SelectReply { selected: None }))
    }

    pub async fn runtime_reload(rm: Arc<RuntimeManager>) -> Result<impl warp::Reply, Infallible> {
        if rm.reload().await.is_ok() {
            Ok(StatusCode::OK)
        } else {
            Ok(StatusCode::ACCEPTED)
        }
    }

    pub async fn runtime_shutdown(rm: Arc<RuntimeManager>) -> Result<impl warp::Reply, Infallible> {
        if rm.shutdown().await {
            Ok(StatusCode::OK)
        } else {
            Ok(StatusCode::ACCEPTED)
        }
    }

    #[cfg(feature = "stat")]
    pub async fn stat_html(rm: Arc<RuntimeManager>) -> Result<impl warp::Reply, Infallible> {
        let mut body = String::from(
            r#"<html>
<head><style>
table, th, td {
  border: 1px solid black;
  border-collapse: collapse;
  text-align: right;
  padding: 4;
  font-size: small;
}
.highlight {
  font-weight: bold;
}
</style></head>
<table style=\"border=4px solid\">
        "#,
        );
        let sm = rm.stat_manager();
        let sm = sm.read().await;
        let total_counters = sm.counters.iter().count();
        let active_counters = sm
            .counters
            .iter()
            .filter(|x| !x.send_completed() || !x.recv_completed())
            .count();
        body.push_str(&format!(
            "Total {}, Active {}<br><br>",
            total_counters, active_counters
        ));
        body.push_str("<tr><td>Network</td><td>Destination</td><td>SentBytes</td><td>RecvdBytes</td><td>SendFin</td><td>RecvFin</td></tr>");
        for c in sm.counters.iter() {
            body.push_str(&format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                &c.sess.network,
                &c.sess.destination,
                c.bytes_sent(),
                c.bytes_recvd(),
                c.send_completed(),
                c.recv_completed(),
            ));
        }
        body.push_str("</table></html>");
        Ok(warp::reply::html(body))
    }
}

mod filters {
    use super::*;

    fn with_runtime_manager(
        rm: Arc<RuntimeManager>,
    ) -> impl Filter<Extract = (Arc<RuntimeManager>,), Error = Infallible> + Clone {
        warp::any().map(move || rm.clone())
    }

    // POST /api/v1/app/outbound/select?outbound=Proxy&select=p3
    pub fn select_update(
        rm: Arc<RuntimeManager>,
    ) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
        warp::path!("api" / "v1" / "app" / "outbound" / "select")
            .and(warp::post())
            .and(warp::query::<models::SelectOptions>())
            .and(with_runtime_manager(rm))
            .and_then(handlers::select_update)
    }

    // GET /api/v1/app/outbound/select?outbound=Proxy
    pub fn select_get(
        rm: Arc<RuntimeManager>,
    ) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
        warp::path!("api" / "v1" / "app" / "outbound" / "select")
            .and(warp::get())
            .and(warp::query::<models::SelectOptions>())
            .and(with_runtime_manager(rm))
            .and_then(handlers::select_get)
    }

    // POST /api/v1/runtime/reload
    pub fn runtime_reload(
        rm: Arc<RuntimeManager>,
    ) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
        warp::path!("api" / "v1" / "runtime" / "reload")
            .and(warp::post())
            .and(with_runtime_manager(rm))
            .and_then(handlers::runtime_reload)
    }

    // POST /api/v1/runtime/shutdown
    pub fn runtime_shutdown(
        rm: Arc<RuntimeManager>,
    ) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
        warp::path!("api" / "v1" / "runtime" / "shutdown")
            .and(warp::post())
            .and(with_runtime_manager(rm))
            .and_then(handlers::runtime_shutdown)
    }

    // POST /api/v1/runtime/stat/html
    #[cfg(feature = "stat")]
    pub fn stat_html(
        rm: Arc<RuntimeManager>,
    ) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
        warp::path!("api" / "v1" / "runtime" / "stat" / "html")
            .and(warp::get())
            .and(with_runtime_manager(rm))
            .and_then(handlers::stat_html)
    }
}

pub struct ApiServer {
    runtime_manager: Arc<RuntimeManager>,
}

impl ApiServer {
    pub fn new(runtime_manager: Arc<RuntimeManager>) -> Self {
        Self { runtime_manager }
    }

    pub fn serve(&self, listen_addr: SocketAddr) -> crate::Runner {
        let routes = filters::select_update(self.runtime_manager.clone())
            .or(filters::select_get(self.runtime_manager.clone()))
            .or(filters::runtime_reload(self.runtime_manager.clone()))
            .or(filters::runtime_shutdown(self.runtime_manager.clone()));

        #[cfg(feature = "stat")]
        let routes = routes.or(filters::stat_html(self.runtime_manager.clone()));

        log::info!("api server listening tcp {}", &listen_addr);
        Box::pin(warp::serve(routes).bind(listen_addr))
    }
}
