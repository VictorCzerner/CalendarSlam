use calendar_slam_shared::{MpClientMsg, MpServerMsg};
use futures_util::{SinkExt, StreamExt};
use gloo_net::websocket::{futures::WebSocket, Message};
use wasm_bindgen_futures::spawn_local;
use yew::Callback;

pub struct MpConnection {
    sender: futures_channel::mpsc::UnboundedSender<MpClientMsg>,
}

impl MpConnection {
    pub fn connect(callback: Callback<Result<MpServerMsg, String>>) -> Result<Self, String> {
        let protocol = web_sys::window()
            .and_then(|window| window.location().protocol().ok())
            .unwrap_or_else(|| "http:".to_string());
        let host = web_sys::window()
            .and_then(|window| window.location().host().ok())
            .ok_or_else(|| "host indisponivel".to_string())?;
        let scheme = if protocol == "https:" { "wss" } else { "ws" };
        let ws = WebSocket::open(&format!("{scheme}://{host}/api/mp/ws"))
            .map_err(|err| err.to_string())?;
        let (mut write, mut read) = ws.split();
        let (sender, mut outbound) = futures_channel::mpsc::unbounded::<MpClientMsg>();

        spawn_local(async move {
            while let Some(message) = outbound.next().await {
                let Ok(text) = serde_json::to_string(&message) else {
                    continue;
                };
                if write.send(Message::Text(text)).await.is_err() {
                    break;
                }
            }
            // The sender was dropped (we left the room): close the socket so the server drops us.
            let _ = write.close().await;
        });

        spawn_local(async move {
            while let Some(message) = read.next().await {
                match message {
                    Ok(Message::Text(text)) => {
                        callback.emit(serde_json::from_str::<MpServerMsg>(&text).map_err(|err| err.to_string()));
                    }
                    Ok(_) => {}
                    Err(err) => callback.emit(Err(err.to_string())),
                }
            }
            callback.emit(Err("Conexao multiplayer encerrada.".to_string()));
        });

        Ok(Self { sender })
    }

    pub fn send(&self, message: MpClientMsg) -> Result<(), String> {
        self.sender
            .unbounded_send(message)
            .map_err(|_| "Conexao multiplayer indisponivel.".to_string())
    }
}
