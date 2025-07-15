use chrono::Utc;
use hydrochat_core::{protocol::Message as ChatMessage, ChatClient};
use js_sys::{Array, Uint8Array};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;
use web_sys::{BinaryType, MessageEvent, WebSocket};

thread_local! {
    static MESSAGE_QUEUE: RefCell<Vec<Vec<u8>>> = RefCell::new(Vec::new());
}

#[wasm_bindgen]
pub struct ChatClientWasm {
    client: ChatClient,
    ws: Option<WebSocket>,
}

#[wasm_bindgen]
impl ChatClientWasm {
    #[wasm_bindgen(constructor)]
    pub fn new(nickname: String) -> Self {
        Self {
            client: ChatClient::new(nickname),
            ws: None,
        }
    }

    #[wasm_bindgen]
    pub async fn connect(&mut self, url: String) -> Result<(), JsValue> {
        let ws = WebSocket::new(&url)?;
        ws.set_binary_type(BinaryType::Arraybuffer);
        let onmessage_callback = Closure::<dyn FnMut(_)>::new(move |e: MessageEvent| {
            if let Ok(abuf) = e.data().dyn_into::<js_sys::ArrayBuffer>() {
                let array = Uint8Array::new(&abuf).to_vec();
                MESSAGE_QUEUE.with(|q| q.borrow_mut().push(array));
            }
        });
        ws.set_onmessage(Some(onmessage_callback.as_ref().unchecked_ref()));
        onmessage_callback.forget();

        // Wait for WebSocket to be open before sending ConnectRequest
        let (tx, rx) = futures_channel::oneshot::channel();
        let tx = std::rc::Rc::new(std::cell::RefCell::new(Some(tx)));
        let onopen = {
            let tx = tx.clone();
            Closure::<dyn FnMut()>::new(move || {
                if let Some(tx) = tx.borrow_mut().take() {
                    let _ = tx.send(Ok(()));
                }
            })
        };
        let onerror = {
            let tx = tx.clone();
            Closure::<dyn FnMut()>::new(move || {
                if let Some(tx) = tx.borrow_mut().take() {
                    let _ = tx.send(Err(JsValue::from_str("WebSocket error before open")));
                }
            })
        };
        let onclose = {
            let tx = tx.clone();
            Closure::<dyn FnMut()>::new(move || {
                if let Some(tx) = tx.borrow_mut().take() {
                    let _ = tx.send(Err(JsValue::from_str("WebSocket closed before open")));
                }
            })
        };
        ws.set_onopen(Some(onopen.as_ref().unchecked_ref()));
        ws.set_onerror(Some(onerror.as_ref().unchecked_ref()));
        ws.set_onclose(Some(onclose.as_ref().unchecked_ref()));
        onopen.forget();
        onerror.forget();
        onclose.forget();

        // Wait for open or error/close
        wasm_bindgen_futures::JsFuture::from(js_sys::Promise::from(
            wasm_bindgen_futures::future_to_promise(async move {
                rx.await
                    .map_err(|_| JsValue::from_str("WebSocket open/close channel dropped"))??;
                Ok(JsValue::UNDEFINED)
            }),
        ))
        .await?;

        // Now send ConnectRequest
        let connect_request = ChatMessage::ConnectRequest;
        let bytes = bincode::serialize(&connect_request)
            .map_err(|e| JsValue::from_str(&format!("Serialization error: {:?}", e)))?;
        ws.send_with_u8_array(&bytes)?;
        self.ws = Some(ws);
        Ok(())
    }

    #[wasm_bindgen]
    pub async fn send_message(&self, message: String) -> Result<(), JsValue> {
        let msg = ChatMessage::ChatMsg {
            nickname: self.client.get_nickname(),
            message,
            // ts: Utc::now(),
        };
        let bytes = bincode::serialize(&msg)
            .map_err(|e| JsValue::from_str(&format!("Serialization error: {:?}", e)))?;
        if let Some(ws) = &self.ws {
            ws.send_with_u8_array(&bytes)?;
            Ok(())
        } else {
            Err(JsValue::from_str("WebSocket not connected"))
        }
    }

    #[wasm_bindgen]
    pub async fn receive_messages(&self) -> Result<Array, JsValue> {
        let out = Array::new();
        MESSAGE_QUEUE.with(|q| {
            let mut q = q.borrow_mut();
            while let Some(bytes) = q.pop() {
                match bincode::deserialize::<ChatMessage>(&bytes) {
                    Ok(ChatMessage::ChatMsg {
                        nickname,
                        message,
                        // ts,
                    }) => {
                        let s = format!("[{}] {}", nickname, message);
                        out.push(&JsValue::from_str(&s));
                    }
                    Ok(ChatMessage::ConnectRequest) => {
                        // Ignore ConnectRequest messages (shouldn't receive these as client)
                    }
                    Ok(ChatMessage::ConnectResponse) => {
                        out.push(&JsValue::from_str("Connection confirmed by server"));
                    }
                    Err(e) => {
                        out.push(&JsValue::from_str(&format!(
                            "Deserialization error: {:?}",
                            e
                        )));
                    }
                }
            }
        });
        Ok(out)
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn run_chat_client(nickname: String, websocket_url: String) {
    spawn_local(async move {
        let mut client = ChatClientWasm::new(nickname);
        if let Err(e) = client.connect(websocket_url).await {
            web_sys::console::log_1(&format!("Failed to connect: {e:?}").into());
            return;
        }
        loop {
            if let Err(e) = client.receive_messages().await {
                web_sys::console::log_1(&format!("Receive error: {e:?}").into());
                break;
            }
            gloo_timers::future::TimeoutFuture::new(100).await;
        }
    });
}
