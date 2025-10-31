mod api;
mod config;
mod intercept;
mod service;

use anyhow::Context;
use axum::{routing::get, Router};
use config::GatewayConfig;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::signal;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

use crate::api::{collab_execute_route, tools_execute_route};
use crate::intercept::InterceptorFacade;
use crate::service::GatewayService;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    init_tracing();

    let config = GatewayConfig::from_env()?;
    let addr: SocketAddr = config.bind_addr.parse().context("解析 SB_GATEWAY_ADDR 失败")?;

    let interceptor = InterceptorFacade::new();
    let service = Arc::new(GatewayService::new(config.clone()));
    let app_state = Arc::new(api::AppState::new(interceptor, service));

    let app = Router::new()
        .route("/healthz", get(api::healthz))
        .route(
            "/tenants/:tenant_id/tools.execute",
            axum::routing::post(tools_execute_route),
        )
        .route(
            "/tenants/:tenant_id/collab.execute",
            axum::routing::post(collab_execute_route),
        )
        .with_state(app_state);

    info!("sb-gateway listening on {addr}");
    axum::serve(tokio::net::TcpListener::bind(addr).await?, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("启动 sb-gateway 失败")?;

    Ok(())
}

fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,sb_gateway=debug")),
        )
        .with_target(false)
        .try_init();
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install CTRL+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    info!("shutdown signal received");
}
