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
    ScanProgress { checked: usize, total: usize },
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
    let queued_chats = Arc::new(AtomicUsize::new(0));
    let checked_chats = Arc::new(AtomicUsize::new(0));

    let sync_tx = tx.clone();
    let sync_seen_jids = seen_jids.clone();
    let sync_pending_batches = pending_batches.clone();
    let sync_queued_chats = queued_chats.clone();
    let sync_checked_chats = checked_chats.clone();

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
            let queued_chats = sync_queued_chats.clone();
            let checked_chats = sync_checked_chats.clone();

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
                queued_chats.fetch_add(new_chats.len(), Ordering::SeqCst);

                tokio::spawn(async move {
                    // One batched usync gives verified business names and a cheap
                    // business flag, so profile IQs only go to likely businesses.
                    let mut user_info = std::collections::HashMap::new();
                    for chunk in new_chats.chunks(50) {
                        let jids: Vec<Jid> = chunk.iter().map(|(jid, _)| jid.clone()).collect();
                        match client.contacts().get_user_info(&jids).await {
                            Ok(infos) => {
                                for (jid, info) in infos {
                                    user_info.insert(jid.to_string(), info);
                                }
                            }
                            Err(error) => {
                                eprintln!("user info query failed: {error}");
                            }
                        }
                        tokio::time::sleep(Duration::from_millis(150)).await;
                    }

                    for (jid, conversation_name) in new_chats {
                        let jid_string = jid.to_string();
                        let info = user_info.remove(&jid_string);

                        let name = conversation_name
                            .or_else(|| {
                                info.as_ref()
                                    .and_then(|info| info.verified_name.as_ref())
                                    .and_then(|verified| verified.name.clone())
                                    .filter(|name| !name.is_empty())
                            })
                            .unwrap_or_else(|| jid.user_base().to_string());

                        let known_non_business = info
                            .as_ref()
                            .is_some_and(|info| !info.is_business && info.business_error.is_none());

                        if !known_non_business {
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
                                    eprintln!(
                                        "business profile query failed for {jid_string}: {error}"
                                    );
                                }
                            }
                        }

                        let checked = checked_chats.fetch_add(1, Ordering::SeqCst) + 1;
                        let _ = tx.send(SessionEvent::ScanProgress {
                            checked,
                            total: queued_chats.load(Ordering::SeqCst),
                        });
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

fn conversation_chat(conversation: &wa::Conversation) -> Option<(Jid, Option<String>)> {
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
        .map(str::to_string);

    Some((jid.into_non_ad(), name))
}
