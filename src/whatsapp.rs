use tokio::sync::mpsc::UnboundedSender;
use whatsapp_rust::prelude::*;

#[derive(Debug, Clone)]
pub enum SessionEvent {
    Qr(String),
    Connected,
    LoggedOut,
    Error(String),
}

pub fn spawn(tx: UnboundedSender<SessionEvent>) {
    std::thread::Builder::new()
        .name("whatsapp".into())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
                Ok(runtime) => runtime,
                Err(error) => {
                    let _ = tx.send(SessionEvent::Error(error.to_string()));
                    return;
                }
            };
            runtime.block_on(run(tx));
        })
        .expect("failed to spawn whatsapp thread");
}

async fn run(tx: UnboundedSender<SessionEvent>) {
    let store = match SqliteStore::new("whatsapp.db").await {
        Ok(store) => store,
        Err(error) => {
            let _ = tx.send(SessionEvent::Error(error.to_string()));
            return;
        }
    };

    let qr_tx = tx.clone();
    let connected_tx = tx.clone();
    let logged_out_tx = tx.clone();

    let bot = Bot::builder()
        .with_backend(store)
        .on_qr_code(move |code, _timeout| {
            let tx = qr_tx.clone();
            async move {
                let _ = tx.send(SessionEvent::Qr(code));
            }
        })
        .on_connected(move |_client| {
            let tx = connected_tx.clone();
            async move {
                let _ = tx.send(SessionEvent::Connected);
            }
        })
        .on_logged_out(move |_info| {
            let tx = logged_out_tx.clone();
            async move {
                let _ = tx.send(SessionEvent::LoggedOut);
            }
        })
        .build()
        .await;

    match bot {
        Ok(bot) => bot.run().await,
        Err(error) => {
            let _ = tx.send(SessionEvent::Error(error.to_string()));
        }
    }
}
