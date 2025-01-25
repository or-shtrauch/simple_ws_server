use std::{env, io::Error, net::SocketAddr, sync::{Arc, Mutex}, time::Duration};
use futures_util::{SinkExt, StreamExt};
use log::info;
use tokio::net::{TcpListener, TcpStream};
use tokio::time::{Instant, interval_at};
use tokio_tungstenite::{tungstenite::{self, Message}, WebSocketStream};

type ConnList = Arc<Mutex<Vec<SocketAddr>>>;

fn create_interval(time: u64) -> tokio::time::Interval {
    interval_at(Instant::now() + Duration::from_secs(time), Duration::from_secs(time))
}

async fn ws_message_handler(msg: &tungstenite::Message,
                        ws_conn: &mut WebSocketStream<TcpStream>)
                        -> tungstenite::Result<()> {
    match msg {
        tokio_tungstenite::tungstenite::Message::Text(txt) => {
            info!("got text: {:?}", txt);
            ws_conn.send(Message::Text(txt.to_string())).await?;
        }
        _msg => {
            info!("got other message type: {:?}", _msg);
        }
    }

    Ok(())
}

async fn connection_handler(stream: TcpStream, conn_list: ConnList) -> tungstenite::Result<()> {
    let addr = stream.peer_addr()?;
    let mut ws_conn = tokio_tungstenite::accept_async(stream).await?;

    let mut ping_interval = create_interval(5);

    let mut timeout_interval = create_interval(20);

    info!("New WebSocket connection: {}", addr);

    if let Ok(mut conn_list) = conn_list.lock() {
        info!("adding {} to conn_list", addr);
        conn_list.push(addr.clone());
    }

    loop {
        tokio::select! {
            _ = timeout_interval.tick() => {
                info!("timeout for {}, closing connection", addr);
                ws_conn.close(None).await?;
                break;
            }
            _ = ping_interval.tick() => {
                info!("sending ping to {}", addr);
                ws_conn.send(tungstenite::Message::Ping(vec![])).await?;
            }
            msg = ws_conn.next() => {
                if let Some(Ok(msg)) = msg {
                    match msg {
                        tungstenite::Message::Close(_) => {
                            info!("got close message");
                            break;
                        }
                        _ => {
                            info!("resetting timeout for {}", addr);
                            timeout_interval.reset();
                            let _ = ws_message_handler(&msg, &mut ws_conn).await;
                        }
                    }
                }
            }
        }
    }

    if let Ok(mut conn_list) = conn_list.lock() {
        conn_list.retain(|x| x != &addr);
    }

    Ok(())
}

async fn show_connections(conn_list: ConnList) {
    let mut interval = create_interval(5);
    loop {
        interval.tick().await;
        if let Ok(conn_list) = conn_list.lock() {
            info!("connections[{}] = {:?}", conn_list.len(), conn_list);
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let addr = env::args().nth(1).unwrap_or_else(|| "127.0.0.1:8080".to_string());
    let listener = TcpListener::bind(&addr).await?;
    let conn_list: ConnList = Arc::new(Mutex::new(Vec::new()));

    pretty_env_logger::init();
    info!("Listening on: {}", addr);

    tokio::spawn(show_connections(Arc::clone(&conn_list)));

    while let Ok((stream, _)) = listener.accept().await {
        tokio::spawn(connection_handler(stream, Arc::clone(&conn_list)));
    }

    Ok(())
}