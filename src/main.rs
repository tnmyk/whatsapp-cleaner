mod text_input;
mod whatsapp;

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use gpui::{
    Animation, AnimationExt as _, App, Bounds, Context, Entity, FontWeight, ImageSource,
    KeyBinding, RenderImage, SharedString, Subscription, TitlebarOptions, Window, WindowBounds,
    WindowOptions, div, img, prelude::*, px, relative, rgb, rgba, size,
};
use gpui_platform::application;
use text_input::{
    Backspace, Copy, Cut, Delete, End, Home, Left, Paste, Right, SelectAll, SelectLeft,
    SelectRight, ShowCharacterPalette,
};
use tokio::sync::mpsc;
use whatsapp::{BusinessChat, Command, DeleteTarget, SessionEvent};

const BG: u32 = 0x0b141a;
const SURFACE: u32 = 0x111b21;
const CARD: u32 = 0x1f2c34;
const BORDER: u32 = 0x2a3942;
const TEXT: u32 = 0xe9edef;
const MUTED: u32 = 0x8696a0;
const ACCENT: u32 = 0x00a884;
const DANGER: u32 = 0xf15c6d;
const DANGER_SURFACE: u32 = 0x2a1a1e;

#[derive(Clone)]
struct FailedDelete {
    target: DeleteTarget,
    name: String,
    error: String,
}

struct WhatsappCleaner {
    qr: Option<Arc<RenderImage>>,
    status: SharedString,
    hint: SharedString,
    chats: Vec<BusinessChat>,
    selected: HashSet<String>,
    search_input: Entity<text_input::TextInput>,
    _search_subscription: Subscription,
    commands: mpsc::UnboundedSender<Command>,
    pending_delete: Option<Vec<DeleteTarget>>,
    failed_delete: Option<FailedDelete>,
    checked: usize,
    total: usize,
    scanning: bool,
    error: bool,
}

impl WhatsappCleaner {
    fn new(cx: &mut Context<Self>, commands: mpsc::UnboundedSender<Command>) -> Self {
        let search_input =
            cx.new(|cx| text_input::TextInput::new("Search by name, category, or phone", cx));
        let _search_subscription = cx.observe(&search_input, |_, _, cx| cx.notify());

        Self {
            qr: None,
            status: "Connecting to WhatsApp…".into(),
            hint: "".into(),
            chats: Vec::new(),
            selected: HashSet::new(),
            search_input,
            _search_subscription,
            commands,
            pending_delete: None,
            failed_delete: None,
            checked: 0,
            total: 0,
            scanning: false,
            error: false,
        }
    }

    fn handle(&mut self, event: SessionEvent) {
        match event {
            SessionEvent::Qr(code) => {
                self.qr = qr_render_image(&code).map(Arc::new);
                self.status = "Scan to link this device".into();
                self.hint = "WhatsApp → Settings → Linked devices → Link a device".into();
                self.error = false;
                self.selected.clear();
            }
            SessionEvent::Connected => {
                self.qr = None;
                self.status = "Scanning chats…".into();
                self.hint = "".into();
                self.error = false;
            }
            SessionEvent::ScanStarted => {
                self.scanning = true;
                self.error = false;
            }
            SessionEvent::ScanProgress { checked, total } => {
                self.checked = checked;
                self.total = total;
                self.scanning = checked < total;
                self.error = false;
            }
            SessionEvent::BusinessChat(chat) => {
                self.scanning = true;
                self.error = false;
                if !self.chats.iter().any(|existing| existing.jid == chat.jid) {
                    self.chats.push(chat);
                }
                self.status = format!(
                    "{} business chat{}",
                    self.chats.len(),
                    if self.chats.len() == 1 { "" } else { "s" }
                )
                .into();
            }
            SessionEvent::ChatDeleted(jid) => {
                self.chats.retain(|chat| chat.jid != jid);
                self.selected.remove(&jid);
                self.hint = "".into();
                self.error = false;
                if self
                    .failed_delete
                    .as_ref()
                    .is_some_and(|failed| failed.target.jid == jid)
                {
                    self.failed_delete = None;
                }

                if self.chats.is_empty() {
                    self.status = "No business chats left".into();
                } else {
                    self.status = format!(
                        "{} business chat{}",
                        self.chats.len(),
                        if self.chats.len() == 1 { "" } else { "s" }
                    )
                    .into();
                }
            }
            SessionEvent::DeleteFailed { jid, error } => {
                if let Some(chat) = self.chats.iter().find(|chat| chat.jid == jid) {
                    self.failed_delete = Some(FailedDelete {
                        target: DeleteTarget {
                            jid: chat.jid.clone(),
                            last_message_timestamp: chat.last_message_timestamp,
                            last_message_key: chat.last_message_key.clone(),
                        },
                        name: chat.name.clone(),
                        error,
                    });
                }
                self.error = false;
            }
            SessionEvent::ScanComplete => {
                self.scanning = false;
                if self.chats.is_empty() {
                    self.status = "No business chats found".into();
                    self.hint = "Only chats delivered in WhatsApp history sync are checked".into();
                }
            }
            SessionEvent::LoggedOut => {
                self.qr = None;
                self.status = "Logged out".into();
                self.hint = "Restart the app to link a device again".into();
                self.error = true;
            }
            SessionEvent::Error(error) => {
                self.qr = None;
                self.status = "WhatsApp error".into();
                self.hint = error.into();
                self.error = true;
            }
        }
    }
}

impl Render for WhatsappCleaner {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let status_color = if self.error {
            rgb(DANGER)
        } else if self.qr.is_some() || self.scanning || !self.chats.is_empty() {
            rgb(ACCENT)
        } else {
            rgb(MUTED)
        };

        let header_label: SharedString = if self.qr.is_some() {
            "Waiting for link".into()
        } else if self.error {
            self.status.clone()
        } else if self.scanning {
            if self.total > 0 {
                format!("Scanning · {} of {}", self.checked, self.total).into()
            } else {
                "Scanning chats…".into()
            }
        } else if !self.chats.is_empty() {
            "Connected".into()
        } else {
            self.status.clone()
        };

        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .w_full()
            .h(px(76.0))
            .px_8()
            .border_b_1()
            .border_color(rgb(BORDER))
            .bg(rgb(SURFACE))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_2xl()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("WhatsApp Cleaner"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(MUTED))
                            .child("Find and manage business chats"),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .max_w(px(420.0))
                    .px_3()
                    .py_2()
                    .rounded_full()
                    .bg(rgb(BG))
                    .border_1()
                    .border_color(rgb(BORDER))
                    .child(div().size(px(8.0)).rounded_full().bg(status_color))
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(MUTED))
                            .truncate()
                            .child(header_label),
                    ),
            );

        let mut body = div()
            .flex()
            .flex_col()
            .items_center()
            .flex_1()
            .min_h(px(0.0))
            .w_full()
            .pt_8()
            .px_8()
            .pb_6();

        if let Some(qr) = &self.qr {
            body = body.child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .flex_1()
                    .gap_5()
                    .child(
                        div()
                            .p_5()
                            .bg(rgb(0xffffff))
                            .rounded_xl()
                            .shadow_lg()
                            .child(img(ImageSource::Render(qr.clone())).size(px(360.0))),
                    )
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::MEDIUM)
                            .child("Scan to link this device"),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(MUTED))
                            .child(self.hint.clone()),
                    ),
            );
        } else if self.chats.is_empty() {
            if self.scanning {
                let mut skeleton_rows = div().flex().flex_col().gap_2().w_full();
                for _ in 0..6 {
                    skeleton_rows = skeleton_rows.child(
                        div()
                            .flex()
                            .items_center()
                            .gap_4()
                            .w_full()
                            .p_4()
                            .rounded_xl()
                            .bg(rgb(CARD))
                            .border_1()
                            .border_color(rgb(BORDER))
                            .child(div().size(px(52.0)).rounded_full().bg(rgb(BORDER)))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_2()
                                    .flex_1()
                                    .child(
                                        div()
                                            .h(px(12.0))
                                            .w(px(190.0))
                                            .rounded_full()
                                            .bg(rgb(BORDER)),
                                    )
                                    .child(
                                        div()
                                            .h(px(10.0))
                                            .w(px(110.0))
                                            .rounded_full()
                                            .bg(rgb(BORDER)),
                                    )
                                    .child(
                                        div()
                                            .h(px(10.0))
                                            .w(px(300.0))
                                            .rounded_full()
                                            .bg(rgb(BORDER)),
                                    ),
                            ),
                    );
                }

                let skeleton = div()
                    .flex()
                    .flex_col()
                    .w(px(760.0))
                    .flex_1()
                    .min_h(px(0.0))
                    .child(skeleton_rows)
                    .with_animation(
                        "skeleton-pulse",
                        Animation::new(Duration::from_millis(1200)).repeat(),
                        |this, delta| this.opacity(0.55 + 0.35 * delta),
                    );

                body = body.child(
                    div()
                        .flex()
                        .items_end()
                        .justify_between()
                        .w(px(760.0))
                        .mb_4()
                        .flex_shrink_0()
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .child(
                                    div()
                                        .text_2xl()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child("Business chats"),
                                )
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(rgb(MUTED))
                                        .child("Fetching chats from history sync…"),
                                ),
                        ),
                );
                body = body.child(skeleton);
            } else {
                let (title, message): (&str, String) = if self.error {
                    ("Something went wrong", self.hint.to_string())
                } else {
                    (
                        "No business chats found",
                        "Only chats delivered in WhatsApp history sync can be checked.".to_string(),
                    )
                };

                body = body.child(
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .gap_3()
                        .w(px(760.0))
                        .flex_1()
                        .max_h(px(420.0))
                        .p_8()
                        .rounded_xl()
                        .bg(rgb(SURFACE))
                        .border_1()
                        .border_color(rgb(BORDER))
                        .child(
                            div()
                                .size(px(44.0))
                                .rounded_full()
                                .bg(rgb(CARD))
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_lg()
                                .text_color(rgb(ACCENT))
                                .child("?"),
                        )
                        .child(div().text_lg().font_weight(FontWeight::MEDIUM).child(title))
                        .child(
                            div()
                                .text_sm()
                                .text_color(rgb(MUTED))
                                .text_center()
                                .max_w(px(420.0))
                                .child(message),
                        ),
                );
            }
        } else {
            let query_raw = self.search_input.read(cx).content().trim().to_string();
            let query = query_raw.to_lowercase();
            let visible_chats: Vec<&BusinessChat> = self
                .chats
                .iter()
                .filter(|chat| matches_query(chat, &query))
                .collect();
            let count = visible_chats.len();
            let subtitle: String = if self.scanning {
                "From your WhatsApp history sync".to_string()
            } else if self.total > 0 {
                format!("{} chats checked", self.total)
            } else {
                "From your WhatsApp history sync".to_string()
            };

            let all_selected = !visible_chats.is_empty()
                && visible_chats
                    .iter()
                    .all(|chat| self.selected.contains(&chat.jid));
            let select_label = if all_selected {
                "Clear all"
            } else {
                "Select all"
            };
            let selectable_jids: Vec<String> =
                visible_chats.iter().map(|chat| chat.jid.clone()).collect();
            let select_all = div()
                .id("select-all")
                .text_xs()
                .text_color(rgb(ACCENT))
                .cursor_pointer()
                .hover(|style| style.text_color(rgb(TEXT)))
                .on_click(cx.listener(move |this, _, _, cx| {
                    select_all_visible(&mut this.selected, &selectable_jids, all_selected);
                    cx.notify();
                }))
                .child(select_label);

            let section = div()
                .flex()
                .items_end()
                .justify_between()
                .w(px(760.0))
                .mb_4()
                .flex_shrink_0()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .text_2xl()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("Business chats"),
                        )
                        .child(div().text_sm().text_color(rgb(MUTED)).child(subtitle)),
                )
                .child(
                    div().flex().items_center().gap_4().child(select_all).child(
                        div()
                            .px_3()
                            .py_1()
                            .rounded_full()
                            .bg(rgb(0x005c4b))
                            .text_xs()
                            .text_color(rgb(TEXT))
                            .child(format!("{count} found",)),
                    ),
                );

            let mut list = div()
                .id("business-chats")
                .flex()
                .flex_col()
                .gap_2()
                .w(px(760.0))
                .flex_1()
                .min_h(px(0.0))
                .overflow_scroll();

            for chat in visible_chats.iter().copied() {
                let initial = chat
                    .name
                    .chars()
                    .next()
                    .map(|c| c.to_uppercase().to_string())
                    .unwrap_or_else(|| "?".into());

                let mut row = div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_4()
                    .w_full()
                    .p_4()
                    .rounded_xl()
                    .bg(rgb(CARD))
                    .border_1()
                    .border_color(rgb(BORDER));

                let checked = self.selected.contains(&chat.jid);
                let checkbox_jid = chat.jid.clone();
                row = row.child(
                    div()
                        .id(format!("select-{}", chat.jid))
                        .size(px(22.0))
                        .rounded_md()
                        .border_1()
                        .border_color(if checked { rgb(ACCENT) } else { rgb(0x3b4a54) })
                        .bg(if checked { rgb(ACCENT) } else { rgb(CARD) })
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_xs()
                        .text_color(rgb(BG))
                        .cursor_pointer()
                        .hover(|style| style.border_color(rgb(ACCENT)))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if !this.selected.remove(&checkbox_jid) {
                                this.selected.insert(checkbox_jid.clone());
                            }
                            cx.notify();
                        }))
                        .child(if checked { "✓" } else { "" }),
                );

                if let Some(avatar) = &chat.avatar {
                    row = row.child(
                        div()
                            .size(px(52.0))
                            .rounded_full()
                            .overflow_hidden()
                            .child(img(ImageSource::Render(avatar.clone())).size(px(52.0))),
                    );
                } else {
                    row = row.child(
                        div()
                            .size(px(52.0))
                            .rounded_full()
                            .bg(rgb(0x005c4b))
                            .flex()
                            .justify_center()
                            .items_center()
                            .text_lg()
                            .child(initial),
                    );
                }

                let mut details = div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .flex_1()
                    .min_w(px(0.0))
                    .child(
                        div()
                            .text_base()
                            .font_weight(FontWeight::SEMIBOLD)
                            .truncate()
                            .child(chat.name.clone()),
                    );

                if !chat.category.is_empty() {
                    details = details.child(
                        div()
                            .mt_1()
                            .self_start()
                            .min_w(px(96.0))
                            .px_3()
                            .py_1()
                            .rounded_full()
                            .bg(rgb(BG))
                            .text_xs()
                            .text_center()
                            .text_color(rgb(ACCENT))
                            .child(chat.category.clone()),
                    );
                }

                if !chat.description.is_empty() {
                    details = details.child(
                        div()
                            .text_sm()
                            .text_color(rgb(0xd1d7db))
                            .line_clamp(2)
                            .child(chat.description.clone()),
                    );
                }

                let delete_target = DeleteTarget {
                    jid: chat.jid.clone(),
                    last_message_timestamp: chat.last_message_timestamp,
                    last_message_key: chat.last_message_key.clone(),
                };
                let delete_tx = self.commands.clone();
                let delete_button = div()
                    .id(format!("delete-{}", chat.jid))
                    .flex()
                    .items_center()
                    .justify_center()
                    .px_4()
                    .py_2()
                    .rounded_lg()
                    .border_1()
                    .border_color(rgb(0x5c2b32))
                    .text_sm()
                    .text_color(rgb(DANGER))
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(DANGER_SURFACE)).border_color(rgb(DANGER)))
                    .on_click(move |_, _, _| {
                        let _ = delete_tx.send(Command::DeleteChats(vec![delete_target.clone()]));
                    })
                    .child("Delete");

                row = row.child(details).child(delete_button);
                list = list.child(row);
            }

            if visible_chats.is_empty() {
                list = list.child(
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .gap_2()
                        .w_full()
                        .py_12()
                        .rounded_xl()
                        .bg(rgb(SURFACE))
                        .border_1()
                        .border_color(rgb(BORDER))
                        .child(
                            div()
                                .text_base()
                                .font_weight(FontWeight::MEDIUM)
                                .child("No matching chats"),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(rgb(MUTED))
                                .child(format!("Nothing matches “{query_raw}”")),
                        ),
                );
            }

            body = body.child(section);

            let has_query = !query.is_empty();
            let search_focused = self.search_input.read(cx).focus_handle().is_focused(window);

            let mut search_bar = div()
                .id("search")
                .w(px(760.0))
                .mb_3()
                .px_4()
                .py_2()
                .flex()
                .items_center()
                .gap_3()
                .rounded_xl()
                .bg(rgb(SURFACE))
                .border_1()
                .border_color(if search_focused {
                    rgb(ACCENT)
                } else {
                    rgb(BORDER)
                })
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .child(self.search_input.clone()),
                );

            if has_query {
                search_bar = search_bar.child(
                    div()
                        .id("clear-search")
                        .px_2()
                        .text_sm()
                        .text_color(rgb(MUTED))
                        .cursor_pointer()
                        .hover(|style| style.text_color(rgb(TEXT)))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.search_input
                                .update(cx, |input, cx| input.clear(window, cx));
                        }))
                        .child("×"),
                );
            }

            body = body.child(search_bar);

            if let Some(failed) = self.failed_delete.clone() {
                let retry_target = failed.target.clone();
                body = body.child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_4()
                        .w(px(760.0))
                        .mb_3()
                        .px_4()
                        .py_3()
                        .rounded_xl()
                        .bg(rgb(DANGER_SURFACE))
                        .border_1()
                        .border_color(rgb(0x5c2b32))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .flex_1()
                                .min_w(px(0.0))
                                .child(
                                    div()
                                        .text_sm()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(rgb(DANGER))
                                        .child(format!("Couldn't delete {}", failed.name)),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(rgb(MUTED))
                                        .line_clamp(2)
                                        .child(failed.error.clone()),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_3()
                                .child(
                                    div()
                                        .id("dismiss-delete-error")
                                        .text_xs()
                                        .text_color(rgb(MUTED))
                                        .cursor_pointer()
                                        .hover(|style| style.text_color(rgb(TEXT)))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.failed_delete = None;
                                            cx.notify();
                                        }))
                                        .child("Dismiss"),
                                )
                                .child(
                                    div()
                                        .id("retry-delete")
                                        .px_3()
                                        .py_2()
                                        .rounded_lg()
                                        .border_1()
                                        .border_color(rgb(0x5c2b32))
                                        .text_xs()
                                        .text_color(rgb(DANGER))
                                        .cursor_pointer()
                                        .hover(|style| {
                                            style.bg(rgb(DANGER_SURFACE)).border_color(rgb(DANGER))
                                        })
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.failed_delete = None;
                                            let _ = this.commands.send(Command::DeleteChats(vec![
                                                retry_target.clone(),
                                            ]));
                                            cx.notify();
                                        }))
                                        .child("Retry"),
                                ),
                        ),
                );
            }

            if !self.selected.is_empty() {
                let selected_count = self.selected.len();
                let selected_targets = delete_targets_for_selection(&self.chats, &self.selected);
                let bulk_bar = div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .w(px(760.0))
                    .mb_3()
                    .px_4()
                    .py_3()
                    .rounded_xl()
                    .bg(rgb(SURFACE))
                    .border_1()
                    .border_color(rgb(BORDER))
                    .flex_shrink_0()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_4()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(format!("{selected_count} selected")),
                            )
                            .child(
                                div()
                                    .id("clear-selection")
                                    .text_xs()
                                    .text_color(rgb(MUTED))
                                    .cursor_pointer()
                                    .hover(|style| style.text_color(rgb(TEXT)))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.selected.clear();
                                        cx.notify();
                                    }))
                                    .child("Clear"),
                            ),
                    );

                let bulk_delete = div()
                    .id("bulk-delete")
                    .px_4()
                    .py_2()
                    .rounded_lg()
                    .border_1()
                    .border_color(rgb(0x5c2b32))
                    .text_sm()
                    .text_color(rgb(DANGER))
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(DANGER_SURFACE)).border_color(rgb(DANGER)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.pending_delete = Some(selected_targets.clone());
                        cx.notify();
                    }))
                    .child("Delete selected");

                let bulk_bar = bulk_bar.child(bulk_delete);
                body = body.child(bulk_bar);
            }

            body = body.child(list);

            if !self.hint.is_empty() {
                body = body.child(
                    div()
                        .mt_4()
                        .text_sm()
                        .text_color(rgb(MUTED))
                        .child(self.hint.clone()),
                );
            }
        }

        let confirm_overlay = self.pending_delete.as_ref().map(|targets| {
            let count = targets.len();

            div()
                .absolute()
                .top(px(0.0))
                .left(px(0.0))
                .size_full()
                .bg(rgba(0x000000aa))
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .w(px(470.0))
                        .p_6()
                        .rounded_xl()
                        .bg(rgb(SURFACE))
                        .border_1()
                        .border_color(rgb(BORDER))
                        .flex()
                        .flex_col()
                        .gap_4()
                        .child(
                            div()
                                .text_xl()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(format!("Delete {count} chat{}?", if count == 1 { "" } else { "s" })),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(rgb(MUTED))
                                .child(
                                    "This removes the conversations and their media from your WhatsApp account. This cannot be undone.",
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .justify_end()
                                .gap_3()
                                .mt_2()
                                .child(
                                    div()
                                        .id("cancel-bulk-delete")
                                        .px_4()
                                        .py_2()
                                        .rounded_lg()
                                        .border_1()
                                        .border_color(rgb(BORDER))
                                        .text_sm()
                                        .text_color(rgb(TEXT))
                                        .cursor_pointer()
                                        .hover(|style| style.bg(rgb(CARD)))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.pending_delete = None;
                                            cx.notify();
                                        }))
                                        .child("Cancel"),
                                )
                                .child(
                                    div()
                                        .id("confirm-bulk-delete")
                                        .px_4()
                                        .py_2()
                                        .rounded_lg()
                                        .bg(rgb(DANGER))
                                        .text_sm()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(rgb(BG))
                                        .cursor_pointer()
                                        .hover(|style| style.opacity(0.85))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            if let Some(targets) = this.pending_delete.take() {
                                                let _ = this.commands.send(Command::DeleteChats(targets));
                                            }
                                            cx.notify();
                                        }))
                                        .child(format!("Delete {count}")),
                                ),
                        ),
                )
        });

        let progress_ratio = if self.total > 0 {
            (self.checked as f32 / self.total as f32).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let show_progress = self.total > 0 && self.checked < self.total;
        let progress_bar = div()
            .w_full()
            .h(px(4.0))
            .bg(rgb(SURFACE))
            .child(div().h_full().w(relative(progress_ratio)).bg(rgb(ACCENT)));

        div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(BG))
            .text_color(rgb(TEXT))
            .child(header)
            .when(show_progress, |this| this.child(progress_bar))
            .child(body)
            .when_some(confirm_overlay, |this, overlay| this.child(overlay))
    }
}

fn qr_render_image(data: &str) -> Option<RenderImage> {
    let code = qrcode::QrCode::new(data.as_bytes()).ok()?;
    let modules = code.width() as u32;
    let quiet = 4;
    let scale = 8;
    let side = (modules + quiet * 2) * scale;

    let mut canvas = image::RgbaImage::from_pixel(side, side, image::Rgba([255, 255, 255, 255]));
    let dark = image::Rgba([17, 27, 33, 255]);

    for (index, color) in code.to_colors().into_iter().enumerate() {
        if color != qrcode::Color::Dark {
            continue;
        }
        let x0 = (index as u32 % modules + quiet) * scale;
        let y0 = (index as u32 / modules + quiet) * scale;
        for y in y0..y0 + scale {
            for x in x0..x0 + scale {
                canvas.put_pixel(x, y, dark);
            }
        }
    }

    Some(RenderImage::new(vec![image::Frame::new(canvas)]))
}

fn matches_query(chat: &BusinessChat, query_lower: &str) -> bool {
    query_lower.is_empty()
        || chat.name.to_lowercase().contains(query_lower)
        || chat.jid.to_lowercase().contains(query_lower)
        || chat.category.to_lowercase().contains(query_lower)
        || chat.description.to_lowercase().contains(query_lower)
}

fn select_all_visible(
    selected: &mut HashSet<String>,
    visible_jids: &[String],
    all_visible_selected: bool,
) {
    if all_visible_selected {
        for jid in visible_jids {
            selected.remove(jid);
        }
    } else {
        selected.extend(visible_jids.iter().cloned());
    }
}

fn delete_targets_for_selection(
    chats: &[BusinessChat],
    selected: &HashSet<String>,
) -> Vec<DeleteTarget> {
    chats
        .iter()
        .filter(|chat| selected.contains(&chat.jid))
        .map(|chat| DeleteTarget {
            jid: chat.jid.clone(),
            last_message_timestamp: chat.last_message_timestamp,
            last_message_key: chat.last_message_key.clone(),
        })
        .collect()
}

fn main() {
    let (events_tx, events_rx) = mpsc::unbounded_channel();
    let (commands_tx, commands_rx) = mpsc::unbounded_channel();
    whatsapp::spawn(events_tx, commands_rx);

    application().run(move |cx: &mut App| {
        cx.bind_keys([
            KeyBinding::new("backspace", Backspace, Some("TextInput")),
            KeyBinding::new("delete", Delete, Some("TextInput")),
            KeyBinding::new("left", Left, Some("TextInput")),
            KeyBinding::new("right", Right, Some("TextInput")),
            KeyBinding::new("shift-left", SelectLeft, Some("TextInput")),
            KeyBinding::new("shift-right", SelectRight, Some("TextInput")),
            KeyBinding::new("cmd-a", SelectAll, Some("TextInput")),
            KeyBinding::new("cmd-v", Paste, Some("TextInput")),
            KeyBinding::new("cmd-c", Copy, Some("TextInput")),
            KeyBinding::new("cmd-x", Cut, Some("TextInput")),
            KeyBinding::new("home", Home, Some("TextInput")),
            KeyBinding::new("end", End, Some("TextInput")),
            KeyBinding::new("ctrl-cmd-space", ShowCharacterPalette, Some("TextInput")),
        ]);

        let bounds = Bounds::centered(None, size(px(1100.0), px(760.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(860.0), px(560.0))),
                titlebar: Some(TitlebarOptions {
                    title: Some("WhatsApp Cleaner".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            move |_, cx| {
                let view = cx.new(|cx| WhatsappCleaner::new(cx, commands_tx.clone()));
                let task_view = view.clone();
                let mut events_rx = events_rx;
                cx.spawn(async move |cx| {
                    while let Some(event) = events_rx.recv().await {
                        task_view.update(cx, |view, cx| {
                            view.handle(event);
                            cx.notify();
                        });
                    }
                })
                .detach();
                view
            },
        )
        .expect("failed to open window");
        cx.activate(true);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chat(
        jid: &str,
        name: &str,
        description: &str,
        category: &str,
        last_message_timestamp: i64,
    ) -> BusinessChat {
        BusinessChat {
            jid: jid.to_string(),
            name: name.to_string(),
            description: description.to_string(),
            category: category.to_string(),
            avatar: None,
            last_message_timestamp,
            last_message_key: None,
        }
    }

    fn jids(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn matches_query_matches_name() {
        let chat = chat("15551234567@s.whatsapp.net", "Acme Bakery", "", "", 0);

        assert!(matches_query(&chat, "acme"));
        assert!(matches_query(&chat, "bakery"));
    }

    #[test]
    fn matches_query_matches_category() {
        let chat = chat("15551234567@s.whatsapp.net", "Acme", "", "Bakery", 0);

        assert!(matches_query(&chat, "bakery"));
    }

    #[test]
    fn matches_query_matches_jid() {
        let chat = chat("15551234567@s.whatsapp.net", "Acme", "", "", 0);

        assert!(matches_query(&chat, "15551234567"));
    }

    #[test]
    fn matches_query_matches_description() {
        let chat = chat(
            "15551234567@s.whatsapp.net",
            "Acme",
            "Fresh bread every morning",
            "",
            0,
        );

        assert!(matches_query(&chat, "fresh bread"));
    }

    #[test]
    fn matches_query_is_case_insensitive() {
        let chat = chat(
            "15551234567@s.whatsapp.net",
            "Acme Bakery",
            "Fresh Bread",
            "Food",
            0,
        );

        assert!(matches_query(&chat, "acme"));
        assert!(matches_query(&chat, "fresh bread"));
        assert!(matches_query(&chat, "food"));
    }

    #[test]
    fn matches_query_empty_query_matches_everything() {
        let chat = chat("15551234567@s.whatsapp.net", "Acme", "", "", 0);

        assert!(matches_query(&chat, ""));
    }

    #[test]
    fn matches_query_rejects_non_match() {
        let chat = chat(
            "15551234567@s.whatsapp.net",
            "Acme Bakery",
            "Fresh bread",
            "Food",
            0,
        );

        assert!(!matches_query(&chat, "hardware"));
    }

    #[test]
    fn select_all_visible_selects_only_visible_jids() {
        let mut selected = HashSet::new();
        let visible = jids(&["a@s.whatsapp.net", "b@s.whatsapp.net"]);

        select_all_visible(&mut selected, &visible, false);

        assert_eq!(selected.len(), 2);
        assert!(selected.contains("a@s.whatsapp.net"));
        assert!(selected.contains("b@s.whatsapp.net"));
    }

    #[test]
    fn select_all_visible_preserves_hidden_selections_when_selecting() {
        let mut selected = HashSet::from(["hidden@s.whatsapp.net".to_string()]);
        let visible = jids(&["a@s.whatsapp.net"]);

        select_all_visible(&mut selected, &visible, false);

        assert!(selected.contains("hidden@s.whatsapp.net"));
        assert!(selected.contains("a@s.whatsapp.net"));
    }

    #[test]
    fn select_all_visible_clears_visible_but_leaves_hidden_selected() {
        let mut selected = HashSet::from([
            "a@s.whatsapp.net".to_string(),
            "b@s.whatsapp.net".to_string(),
            "hidden@s.whatsapp.net".to_string(),
        ]);
        let visible = jids(&["a@s.whatsapp.net", "b@s.whatsapp.net"]);

        select_all_visible(&mut selected, &visible, true);

        assert!(!selected.contains("a@s.whatsapp.net"));
        assert!(!selected.contains("b@s.whatsapp.net"));
        assert!(selected.contains("hidden@s.whatsapp.net"));
        assert_eq!(selected.len(), 1);
    }

    #[test]
    fn select_all_visible_does_not_touch_unrelated_jids() {
        let mut selected = HashSet::from(["unrelated@s.whatsapp.net".to_string()]);
        let visible = jids(&["a@s.whatsapp.net"]);

        select_all_visible(&mut selected, &visible, true);

        assert_eq!(
            selected,
            HashSet::from(["unrelated@s.whatsapp.net".to_string()])
        );
    }

    #[test]
    fn delete_targets_for_selection_only_returns_selected_chats() {
        let chats = vec![
            chat("a@s.whatsapp.net", "A", "", "", 10),
            chat("b@s.whatsapp.net", "B", "", "", 20),
            chat("c@s.whatsapp.net", "C", "", "", 30),
        ];
        let selected = HashSet::from([
            "a@s.whatsapp.net".to_string(),
            "c@s.whatsapp.net".to_string(),
        ]);

        let targets = delete_targets_for_selection(&chats, &selected);

        assert_eq!(targets.len(), 2);
        assert_eq!(targets[0].jid, "a@s.whatsapp.net");
        assert_eq!(targets[1].jid, "c@s.whatsapp.net");
    }

    #[test]
    fn delete_targets_for_selection_preserves_chat_order() {
        let chats = vec![
            chat("a@s.whatsapp.net", "A", "", "", 10),
            chat("b@s.whatsapp.net", "B", "", "", 20),
            chat("c@s.whatsapp.net", "C", "", "", 30),
        ];
        let selected = HashSet::from([
            "c@s.whatsapp.net".to_string(),
            "a@s.whatsapp.net".to_string(),
        ]);

        let targets = delete_targets_for_selection(&chats, &selected);

        assert_eq!(
            targets
                .iter()
                .map(|target| target.jid.as_str())
                .collect::<Vec<_>>(),
            vec!["a@s.whatsapp.net", "c@s.whatsapp.net"]
        );
    }

    #[test]
    fn delete_targets_for_selection_keeps_timestamp() {
        let chats = vec![chat("a@s.whatsapp.net", "A", "", "", 1234)];
        let selected = HashSet::from(["a@s.whatsapp.net".to_string()]);

        let targets = delete_targets_for_selection(&chats, &selected);

        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].last_message_timestamp, 1234);
    }

    #[test]
    fn delete_targets_for_selection_empty_when_nothing_selected() {
        let chats = vec![chat("a@s.whatsapp.net", "A", "", "", 10)];

        let targets = delete_targets_for_selection(&chats, &HashSet::new());

        assert!(targets.is_empty());
    }

    #[test]
    fn qr_render_image_produces_square_pixels() {
        let image = qr_render_image("https://example.com/qr-sample-payload")
            .expect("sample payload should render as a QR image");
        let size = image.size(0);

        assert!(size.width.0 > 0);
        assert!(size.height.0 > 0);
        assert_eq!(size.width, size.height);
        assert!(!image.as_bytes(0).expect("first frame exists").is_empty());
    }
}
