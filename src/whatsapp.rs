use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use gpui::RenderImage;
use tokio::sync::Mutex;
use tokio::sync::mpsc::UnboundedSender;
use whatsapp_rust::prelude::*;
use whatsapp_rust::{
    AppStateResyncMode, MessageId, MessageRef, ProfilePictureRequest, ProfilePictureTarget,
    ProfilePictureType, WAPatchName, message_range,
};

#[derive(Debug, Clone)]
pub struct BusinessChat {
    pub jid: String,
    pub name: String,
    pub description: String,
    pub category: String,
    pub avatar: Option<Arc<RenderImage>>,
    pub last_message_timestamp: i64,
    pub last_message_key: Option<wa::MessageKey>,
}

#[derive(Debug, Clone)]
pub struct DeleteTarget {
    pub jid: String,
    pub last_message_timestamp: i64,
    pub last_message_key: Option<wa::MessageKey>,
}

#[derive(Debug, Clone)]
pub enum Command {
    DeleteChats(Vec<DeleteTarget>),
}

#[derive(Debug, Clone)]
pub enum SessionEvent {
    Qr(String),
    Connected,
    ScanStarted,
    ScanProgress { checked: usize, total: usize },
    BusinessChat(BusinessChat),
    ChatDeleted(String),
    DeleteFailed { jid: String, error: String },
    ScanComplete,
    LoggedOut,
    Error(String),
}

pub fn spawn(
    tx: UnboundedSender<SessionEvent>,
    commands: tokio::sync::mpsc::UnboundedReceiver<Command>,
) {
    std::thread::Builder::new()
        .name("whatsapp".into())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    let _ = tx.send(SessionEvent::Error(error.to_string()));
                    return;
                }
            };
            runtime.block_on(run(tx, commands));
        })
        .expect("failed to spawn whatsapp thread");
}

async fn run(
    tx: UnboundedSender<SessionEvent>,
    commands: tokio::sync::mpsc::UnboundedReceiver<Command>,
) {
    // Ephemeral on-disk store: SQLite snapshots are unsupported for :memory:, so use a
    // fixed temp file that is wiped at startup. Every launch starts unpaired.
    let db_path = std::env::temp_dir().join("whatsapp-cleaner-session.db");
    let db_path_string = db_path.to_string_lossy().into_owned();
    for suffix in ["", "-shm", "-wal"] {
        let _ = std::fs::remove_file(format!("{db_path_string}{suffix}"));
    }

    let store = match SqliteStore::open(&db_path_string).await {
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
                        let Some((jid, name, last_message_timestamp, last_message_key)) =
                            conversation_chat(conversation)
                        else {
                            continue;
                        };
                        if seen_jids.insert(jid.to_string()) {
                            new_chats.push((
                                jid,
                                name,
                                last_message_timestamp,
                                last_message_key,
                            ));
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
                        let jids: Vec<Jid> = chunk.iter().map(|(jid, ..)| jid.clone()).collect();
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

                    for (jid, conversation_name, last_message_timestamp, last_message_key) in
                        new_chats
                    {
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
                                        .pictures()
                                        .lookup(ProfilePictureRequest::new(
                                            ProfilePictureTarget::Contact(&jid),
                                            ProfilePictureType::Preview,
                                        ))
                                        .await
                                    {
                                        Ok(lookup) => lookup
                                            .into_found()
                                            .and_then(|picture| fetch_avatar(&picture.url)),
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
                                        last_message_timestamp,
                                        last_message_key,
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
        Ok(bot) => {
            let handle = bot.spawn();
            let client = handle.client();

            tokio::spawn(async move {
                let mut commands = commands;
                while let Some(command) = commands.recv().await {
                    match command {
                        Command::DeleteChats(jids) => {
                            // Catch the local regular_high mirror up before writing.
                            // The internal conflict loop can only rebuild against the
                            // version this client knows; if bootstrap left it behind,
                            // every attempt loses the same race.
                            let mut caught_up = match client
                                .resync_app_state(
                                    [WAPatchName::RegularHigh],
                                    AppStateResyncMode::Incremental,
                                )
                                .await
                            {
                                Ok(report) => report.all_synced(),
                                Err(error) => {
                                    eprintln!("app-state resync before delete failed: {error}");
                                    false
                                }
                            };

                            if !caught_up {
                                match client
                                    .resync_app_state(
                                        [WAPatchName::RegularHigh],
                                        AppStateResyncMode::Snapshot,
                                    )
                                    .await
                                {
                                    Ok(report) => {
                                        caught_up = report.all_synced();
                                        if !caught_up {
                                            eprintln!(
                                                "app-state snapshot rebuild still incomplete before delete"
                                            );
                                        }
                                    }
                                    Err(error) => {
                                        eprintln!(
                                            "app-state snapshot rebuild before delete failed: {error}"
                                        );
                                    }
                                }
                            }

                            for target in jids {
                                let jid_string = target.jid.clone();
                                let Ok(jid) = jid_string.parse::<Jid>() else {
                                    continue;
                                };
                                let timestamp = if target.last_message_timestamp > 0 {
                                    target.last_message_timestamp
                                } else {
                                    std::time::SystemTime::now()
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .map(|elapsed| elapsed.as_secs() as i64)
                                        .unwrap_or(0)
                                };
                                let mut messages = Vec::new();
                                if let Some(key) = target.last_message_key.as_ref()
                                    && let Some(id) = key.id.as_deref()
                                    && let Ok(message_id) = MessageId::new(id)
                                    && let Ok(reference) = MessageRef::new(
                                        &jid,
                                        message_id,
                                        None,
                                        key.from_me.unwrap_or(false),
                                    )
                                {
                                    messages.push((reference, timestamp));
                                }
                                let range = message_range(timestamp, None, messages);

                                // App-state version races outlive history sync: our patch
                                // base keeps losing to other devices while the initial
                                // syncd collection settles. Retry the whole send after a
                                // backoff instead of surfacing the library's 5-attempt
                                // internal conflict immediately.
                                let mut last_error = String::new();
                                let mut delay = Duration::from_secs(1);
                                let mut deleted = false;

                                for attempt in 0..4 {
                                    match client
                                        .chat_actions()
                                        .delete_chat(&jid, true, Some(range.clone()))
                                        .await
                                    {
                                        Ok(()) => {
                                            deleted = true;
                                            break;
                                        }
                                        Err(error) => {
                                            last_error = error.to_string();
                                            if !last_error.contains("still conflicting") {
                                                break;
                                            }
                                            if attempt < 3 {
                                                tokio::time::sleep(delay).await;
                                                delay *= 2;
                                            }
                                        }
                                    }
                                }

                                if deleted {
                                    let _ = tx.send(SessionEvent::ChatDeleted(jid_string));
                                } else {
                                    let _ = tx.send(SessionEvent::DeleteFailed {
                                        jid: jid_string,
                                        error: last_error,
                                    });
                                }
                            }
                        }
                    }
                }
            });

            handle.await;
        }
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

fn conversation_chat(
    conversation: &wa::Conversation,
) -> Option<(Jid, Option<String>, i64, Option<wa::MessageKey>)> {
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

    let mut last_message_timestamp = conversation
        .last_msg_timestamp
        .or(conversation.conversation_timestamp)
        .unwrap_or(0) as i64;
    let mut last_message_key = None;

    for history_message in &conversation.messages {
        let Some(message) = history_message.message.as_option() else {
            continue;
        };
        let timestamp = message.message_timestamp.unwrap_or(0) as i64;
        if timestamp >= last_message_timestamp
            && let Some(key) = message.key.as_option()
        {
            last_message_timestamp = timestamp;
            last_message_key = Some(key.clone());
        }
    }

    Some((
        jid.into_non_ad(),
        name,
        last_message_timestamp,
        last_message_key,
    ))
}

#[cfg(test)]
mod tests {
    use super::conversation_chat;
    use whatsapp_rust::prelude::*;

    fn conversation(id: &str) -> wa::Conversation {
        let mut conversation = wa::Conversation::default();
        conversation.id = id.to_string();
        conversation
    }

    fn message_key(key_id: &str) -> wa::MessageKey {
        let mut key = wa::MessageKey::default();
        key.from_me = Some(true);
        key.id = Some(key_id.to_string());
        key
    }

    fn web_message_info(timestamp: u64, key_id: &str) -> wa::WebMessageInfo {
        let mut message = wa::WebMessageInfo::default();
        message.key = MessageField::some(message_key(key_id));
        message.message_timestamp = Some(timestamp);
        message
    }

    fn history_message(timestamp: u64, key_id: &str) -> wa::HistorySyncMsg {
        let mut message = wa::HistorySyncMsg::default();
        message.message = MessageField::some(web_message_info(timestamp, key_id));
        message
    }

    #[test]
    fn group_jid_is_skipped() {
        assert!(conversation_chat(&conversation("123456789@g.us")).is_none());
    }

    #[test]
    fn newsletter_status_and_broadcast_jids_are_skipped() {
        for jid in [
            "123456789@newsletter",
            "status@broadcast",
            "123456789@broadcast",
        ] {
            assert!(
                conversation_chat(&conversation(jid)).is_none(),
                "expected {jid} to be skipped"
            );
        }
    }

    #[test]
    fn dm_prefers_pn_jid_over_lid_and_strips_device() {
        let mut conversation = conversation("987654321@lid");
        conversation.pn_jid = Some("15551234567:3@s.whatsapp.net".to_string());

        let (jid, ..) = conversation_chat(&conversation).expect("DM should produce a chat");

        assert_eq!(jid.to_string(), "15551234567@s.whatsapp.net");
    }

    #[test]
    fn dm_without_pn_jid_uses_lid_unchanged() {
        let conversation = conversation("987654321@lid");

        let (jid, ..) = conversation_chat(&conversation).expect("DM should produce a chat");

        assert_eq!(jid.to_string(), "987654321@lid");
    }

    #[test]
    fn conversation_id_device_is_stripped() {
        let conversation = conversation("15551234567:3@s.whatsapp.net");

        let (jid, ..) = conversation_chat(&conversation).expect("DM should produce a chat");

        assert_eq!(jid.to_string(), "15551234567@s.whatsapp.net");
    }

    #[test]
    fn display_name_is_preferred_over_name() {
        let mut conversation = conversation("15551234567@s.whatsapp.net");
        conversation.display_name = Some("Display Name".to_string());
        conversation.name = Some("Fallback Name".to_string());

        let (_, name, ..) = conversation_chat(&conversation).expect("DM should produce a chat");

        assert_eq!(name.as_deref(), Some("Display Name"));
    }

    #[test]
    fn name_is_used_when_display_name_is_missing() {
        let mut conversation = conversation("15551234567@s.whatsapp.net");
        conversation.name = Some("Fallback Name".to_string());

        let (_, name, ..) = conversation_chat(&conversation).expect("DM should produce a chat");

        assert_eq!(name.as_deref(), Some("Fallback Name"));
    }

    #[test]
    fn name_is_none_when_display_name_and_name_are_empty() {
        let mut conversation = conversation("15551234567@s.whatsapp.net");
        conversation.display_name = Some(String::new());
        conversation.name = Some(String::new());

        let (_, name, ..) = conversation_chat(&conversation).expect("DM should produce a chat");

        assert_eq!(name, None);
    }

    #[test]
    fn last_msg_timestamp_is_used_when_there_are_no_messages() {
        let mut conversation = conversation("15551234567@s.whatsapp.net");
        conversation.last_msg_timestamp = Some(1_700_000_000);
        conversation.conversation_timestamp = Some(1_600_000_000);

        let (_, _, timestamp, key) =
            conversation_chat(&conversation).expect("DM should produce a chat");

        assert_eq!(timestamp, 1_700_000_000);
        assert!(key.is_none());
    }

    #[test]
    fn conversation_timestamp_is_used_when_last_msg_timestamp_is_missing() {
        let mut conversation = conversation("15551234567@s.whatsapp.net");
        conversation.conversation_timestamp = Some(1_600_000_000);

        let (_, _, timestamp, key) =
            conversation_chat(&conversation).expect("DM should produce a chat");

        assert_eq!(timestamp, 1_600_000_000);
        assert!(key.is_none());
    }

    #[test]
    fn latest_message_supplies_timestamp_and_key() {
        let mut conversation = conversation("15551234567@s.whatsapp.net");
        conversation.last_msg_timestamp = Some(100);
        conversation.messages = vec![
            history_message(200, "middle-key"),
            history_message(500, "newest-key"),
            history_message(400, "older-key"),
        ];

        let (_, _, timestamp, key) =
            conversation_chat(&conversation).expect("DM should produce a chat");

        assert_eq!(timestamp, 500);
        let key = key.expect("latest message should supply a key");
        assert_eq!(key.id.as_deref(), Some("newest-key"));
        assert_eq!(key.from_me, Some(true));
    }
}
