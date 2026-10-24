use axum::{
    extract::{ws::WebSocket, State, WebSocketUpgrade},
    response::IntoResponse,
    routing::get,
    Router,
};
use bytes::Bytes;
use mimalloc::MiMalloc;
use std::{
    net::SocketAddr,
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        Arc,
    },
};
use tokio::{net::UnixListener, sync::broadcast};
use tracing::info;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

struct AppState {
    tx: broadcast::Sender<Bytes>,
    active_connections: AtomicUsize,
    ipc_messages_received: AtomicU64,
    ws_messages_sent: AtomicU64,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let (tx, _rx) = broadcast::channel::<Bytes>(2048);
    let state = Arc::new(AppState {
        tx: tx.clone(),
        active_connections: AtomicUsize::new(0),
        ipc_messages_received: AtomicU64::new(0),
        ws_messages_sent: AtomicU64::new(0),
    });

    // 1. IPC Unix Domain Socket Listener
    let ipc_state = state.clone();
    tokio::spawn(async move {
        let socket_path = std::env::var("EDGE_IPC_SOCK")
            .unwrap_or_else(|_| "/tmp/supersonic.sock".to_string());
        let _ = std::fs::remove_file(&socket_path);

        let listener = UnixListener::bind(&socket_path)
            .unwrap_or_else(|e| panic!("Failed to bind Unix Domain Socket at {socket_path}: {e}"));
        info!("🔌 IPC Listener active on {}", socket_path);

        loop {
            if let Ok((mut socket, _)) = listener.accept().await {
                let ipc_tx = ipc_state.tx.clone();
                let ipc_counter = ipc_state.clone();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 65536];
                    use tokio::io::AsyncReadExt;
                    while let Ok(n) = socket.read(&mut buf).await {
                        if n == 0 { break; }
                        let payload = Bytes::copy_from_slice(&buf[..n]);
                        ipc_counter.ipc_messages_received.fetch_add(1, Ordering::Relaxed);
                        let _ = ipc_tx.send(payload);
                    }
                });
            }
        }
    });

    // 2. HTTP & WebSocket Router
    let app = Router::new()
        .route("/health", get(|| async { "OK" }))
        .route("/metrics", get(metrics_handler))
        .route("/ws", get(ws_handler))
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], 8091));
    info!("🚀 Supersonic Rust Edge Gateway active on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn metrics_handler(State(state): State<Arc<AppState>>) -> String {
    format!(
        "# HELP supersonic_edge_connected_clients Active WebSocket connections\n\
         # TYPE supersonic_edge_connected_clients gauge\n\
         supersonic_edge_connected_clients {}\n\n\
         # HELP supersonic_edge_ipc_messages_total Total IPC messages received from Go\n\
         # TYPE supersonic_edge_ipc_messages_total counter\n\
         supersonic_edge_ipc_messages_total {}\n\n\
         # HELP supersonic_edge_broadcast_messages_total Total WebSocket messages sent to clients\n\
         # TYPE supersonic_edge_broadcast_messages_total counter\n\
         supersonic_edge_broadcast_messages_total {}\n",
        state.active_connections.load(Ordering::Relaxed),
        state.ipc_messages_received.load(Ordering::Relaxed),
        state.ws_messages_sent.load(Ordering::Relaxed)
    )
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: Arc<AppState>) {
    use axum::extract::ws::Message;
    
    state.active_connections.fetch_add(1, Ordering::Relaxed);
    let mut rx = state.tx.subscribe();

    while let Ok(msg_bytes) = rx.recv().await {
        let text_payload = match String::from_utf8(msg_bytes.to_vec()) {
            Ok(s) => s,
            Err(_) => continue,
        };
        if socket.send(Message::Text(text_payload.into())).await.is_err() {
            break;
        }
        state.ws_messages_sent.fetch_add(1, Ordering::Relaxed);
    }

    state.active_connections.fetch_sub(1, Ordering::Relaxed);
}
