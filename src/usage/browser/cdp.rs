//! Talking to Chrome over its DevTools WebSocket.

use super::*;

pub(super) type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

pub(super) struct Cdp {
    ws: Socket,
    next: u64,
}

impl Cdp {
    pub(super) async fn connect(url: &str) -> Result<Self> {
        let (ws, _) = tokio_tungstenite::connect_async(url)
            .await
            .context("connecting to the keymule browser")?;
        Ok(Self { ws, next: 0 })
    }

    pub(super) async fn call(
        &mut self,
        method: &str,
        params: Value,
        session: Option<&str>,
    ) -> Result<Value> {
        self.next += 1;
        let id = self.next;
        let mut msg = json!({ "id": id, "method": method, "params": params });
        if let Some(s) = session {
            msg["sessionId"] = s.into();
        }
        self.ws.send(Message::Text(msg.to_string().into())).await?;
        let wait = async {
            while let Some(m) = self.ws.next().await {
                let Message::Text(t) = m? else { continue };
                let v: Value = serde_json::from_str(&t)?;
                if v["id"].as_u64() != Some(id) {
                    continue; // an event, or another reply
                }
                if let Some(e) = v.get("error") {
                    bail!("{}", e["message"].as_str().unwrap_or("Chrome refused"));
                }
                return Ok(v["result"].clone());
            }
            bail!("the keymule browser closed the connection")
        };
        tokio::time::timeout(Duration::from_secs(20), wait)
            .await
            .map_err(|_| anyhow!("the keymule browser didn't answer"))?
    }

    /// Run `fill.js`'s function in the tab with `arg`, returning its result.
    /// None when the page was navigating (its context went away).
    pub(super) async fn run(
        &mut self,
        session: &str,
        func: &str,
        arg: &Value,
    ) -> Result<Option<Value>> {
        let expr = format!("({func})({arg})");
        let r = self
            .call(
                "Runtime.evaluate",
                json!({ "expression": expr, "awaitPromise": true, "returnByValue": true }),
                Some(session),
            )
            .await;
        match r {
            Ok(v) if v.get("exceptionDetails").is_some() => Ok(None),
            Ok(v) => Ok(Some(v["result"]["value"].clone())),
            Err(e) if e.to_string().contains("context") => Ok(None),
            Err(e) => Err(e),
        }
    }
}
