//! HTTP server: GET /rpf/readVehicle?v=<name> -> GVEH wire bytes.
//! The archive + keys are loaded ONCE at startup and shared across requests.

use crate::archive::{Archive, GtaKeys};
use crate::pipeline;
use anyhow::Result;
use axum::{
    extract::{Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use std::{collections::HashMap, net::SocketAddr, sync::Arc};

struct AppState {
    veh: Archive,
    keys: GtaKeys,
}

pub fn serve(veh: Archive, keys: GtaKeys, addr: SocketAddr) -> Result<()> {
    let state = Arc::new(AppState { veh, keys });
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    rt.block_on(async move {
        let app = Router::new()
            .route("/health", get(|| async { "ok" }))
            .route("/rpf/readVehicle", get(read_vehicle))
            .with_state(state);
        let listener = tokio::net::TcpListener::bind(addr).await?;
        println!("serving on http://{addr}  (try /rpf/readVehicle?v=zion_hi)");
        axum::serve(listener, app).await?;
        anyhow::Ok(())
    })
}

async fn read_vehicle(
    State(state): State<Arc<AppState>>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let v = match q.get("v") {
        Some(v) if !v.is_empty() => v.clone(),
        _ => return (StatusCode::BAD_REQUEST, "missing ?v=<vehicle>").into_response(),
    };
    let with_tex = q.get("tex").map(|s| s == "1").unwrap_or(false);
    println!("readVehicle v={v} tex={with_tex}");

    let res = tokio::task::spawn_blocking(move || {
        pipeline::build_vehicle(&state.veh, &state.keys, &v, with_tex)
    })
    .await;

    match res {
        Ok(Ok(bytes)) => (
            [(header::CONTENT_TYPE, "application/octet-stream")],
            bytes,
        )
            .into_response(),
        Ok(Err(e)) => (StatusCode::NOT_FOUND, format!("decode error: {e:#}")).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("task error: {e}")).into_response(),
    }
}
