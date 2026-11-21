use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use gpui::RenderImage;
use tokio::sync::Mutex;
use tokio::sync::mpsc::UnboundedSender;
use whatsapp_rust::prelude::*;

#[derive(Debug, Clone)]
pub struct BusinessChat {
    pub jid: String,
    pub name: String,
    pub description: String,
    pub category: String,
    pub avatar: Option<Arc<RenderImage>>,
}

#[derive(Debug, Clone)]
pub enum SessionEvent {
    Qr(String),
    Connected,
    ScanStarted,
    BusinessChat(BusinessChat),
    ScanComplete,
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
    // Ephemeral on-disk store: SQLite snapshots are unsupported for :memory:, so use a
    // fixed temp file that is wiped at startup. Every launch starts unpaired.
    let db_path = std::env::temp_dir().join("whatsapp-cleaner-session.db");
    let db_path_string = db_path.to_string_lossy().into_owned();
    for suffix in ["", "-shm", "-wal"] {
        let _ = std::fs::remove_file(format!("{db_path_string}{suffix}"));
    }

    let store = match SqliteStore::new(&db_path_string).await {
        Ok(store) => store,
        Err(error) => {
            let _ = tx.send(SessionEvent::Error(error.to_string()));
            return;
        }
    };

    let qr_tx = tx.clone();
    let connected_tx = tx.clone();
    let logged_out_tx = tx.clone();

    let seen_jids = Arc::new(Mutex::new(HashSet::<String>::new()));
    let pending_batches = Arc::new(AtomicUsize::new(0));

    let sync_tx = tx.clone();
    let sync_seen_jids = seen_jids.clone();
    let sync_pending_batches = pending_batches.clone();

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
        .on_event_for(&[EventKind::HistorySync], move |event, client| {
            let tx = sync_tx.clone();
            let seen_jids = sync_seen_jids.clone();
            let pending_batches = sync_pending_batches.clone();

            async move {
                let Event::HistorySync(sync) = &*event else {
                    return;
                };
                let Some(history) = sync.get() else {
                    return;
                };

                let mut new_chats = Vec::new();
                {
                    let mut seen_jids = seen_jids.lock().await;
                    for conversation in &history.conversations {
                        let Some((jid, name)) = conversation_chat(conversation) else {
                            continue;
                        };
                        if seen_jids.insert(jid.to_string()) {
                            new_chats.push((jid, name));
                        }
                    }
                }

                if new_chats.is_empty() {
                    return;
                }

                let _ = tx.send(SessionEvent::ScanStarted);
                pending_batches.fetch_add(1, Ordering::SeqCst);

                tokio::spawn(async move {
                    for (jid, name) in new_chats {
                        let jid_string = jid.to_string();
                        match client.get_business_profile(&jid).await {
                            Ok(Some(profile)) => {
                                let avatar = match client
                                    .contacts()
                                    .get_profile_picture(&jid, true)
                                    .await
                                {
                                    Ok(Some(picture)) => fetch_avatar(&picture.url),
                                    Ok(None) => None,
                                    Err(error) => {
                                        eprintln!(
                                            "profile picture query failed for {jid_string}: {error}"
                                        );
                                        None
                                    }
                                };

                                let _ = tx.send(SessionEvent::BusinessChat(BusinessChat {
                                    jid: jid_string,
                                    name,
                                    description: profile.description,
                                    category: profile
                                        .categories
                                        .first()
                                        .map(|category| category.name.clone())
                                        .unwrap_or_default(),
                                    avatar,
                                }));
                            }
                            Ok(None) => {}
                            Err(error) => {
                                eprintln!("business profile query failed for {jid_string}: {error}");
                            }
                        }
                        tokio::time::sleep(Duration::from_millis(150)).await;
                    }

                    if pending_batches.fetch_sub(1, Ordering::SeqCst) == 1 {
                        let _ = tx.send(SessionEvent::ScanComplete);
                    }
                });
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

fn fetch_avatar(url: &str) -> Option<Arc<RenderImage>> {
    if url.is_empty() {
        return None;
    }
    let mut response = ureq::get(url).call().ok()?;
    let bytes = response.body_mut().read_to_vec().ok()?;
    let decoded = image::load_from_memory(&bytes).ok()?.to_rgba8();
    Some(Arc::new(RenderImage::new(vec![image::Frame::new(decoded)])))
}

fn conversation_chat(conversation: &wa::Conversation) -> Option<(Jid, String)> {
    let raw_jid = conversation
        .pn_jid
        .as_deref()
        .filter(|jid| !jid.is_empty())
        .unwrap_or(&conversation.id);
    let jid: Jid = raw_jid.parse().ok()?;

    if !(jid.is_pn() || jid.is_lid()) {
        return None;
    }

    let name = conversation
        .display_name
        .as_deref()
        .or(conversation.name.as_deref())
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| jid.user_base().to_string());

    Some((jid.into_non_ad(), name))
}
