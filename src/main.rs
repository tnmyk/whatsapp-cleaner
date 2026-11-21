mod whatsapp;

use std::sync::Arc;

use gpui::{
    App, Bounds, Context, FontWeight, ImageSource, RenderImage, SharedString, TitlebarOptions,
    Window, WindowBounds, WindowOptions, div, img, prelude::*, px, rgb, size,
};
use gpui_platform::application;
use tokio::sync::mpsc;
use whatsapp::{BusinessChat, SessionEvent};

const BG: u32 = 0x0b141a;
const SURFACE: u32 = 0x111b21;
const CARD: u32 = 0x1f2c34;
const BORDER: u32 = 0x2a3942;
const TEXT: u32 = 0xe9edef;
const MUTED: u32 = 0x8696a0;
const ACCENT: u32 = 0x00a884;
const DANGER: u32 = 0xf15c6d;
const DANGER_SURFACE: u32 = 0x2a1a1e;

struct WhatsappCleaner {
    qr: Option<Arc<RenderImage>>,
    status: SharedString,
    hint: SharedString,
    chats: Vec<BusinessChat>,
    scanning: bool,
    error: bool,
}

impl WhatsappCleaner {
    fn new() -> Self {
        Self {
            qr: None,
            status: "Connecting to WhatsApp…".into(),
            hint: "".into(),
            chats: Vec::new(),
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
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let status_color = if self.error {
            rgb(DANGER)
        } else if self.qr.is_some() || self.scanning {
            rgb(ACCENT)
        } else {
            rgb(MUTED)
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
                            .child(self.status.clone()),
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
            .px_8();

        if let Some(qr) = &self.qr {
            body = body.child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
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
            let (title, message) = if self.scanning {
                (
                    "Looking through your chats",
                    "History sync is still arriving. Business chats will appear as they are checked.",
                )
            } else if self.error {
                ("Something went wrong", self.hint.as_ref())
            } else {
                (
                    "No business chats found",
                    "Only chats delivered in WhatsApp history sync can be checked.",
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
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::MEDIUM)
                            .child(title),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(MUTED))
                            .text_center()
                            .max_w(px(420.0))
                            .child(message.to_string()),
                    ),
            );
        } else {
            let count = self.chats.len();
            let subtitle = if self.scanning {
                "Checking chats from history sync…"
            } else {
                "From your WhatsApp history sync"
            };

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
                    div()
                        .px_3()
                        .py_1()
                        .rounded_full()
                        .bg(rgb(0x005c4b))
                        .text_xs()
                        .text_color(rgb(TEXT))
                        .child(format!(
                            "{count} found",
                        )),
                );

            let mut list = div()
                .id("business-chats")
                .flex()
                .flex_col()
                .gap_2()
                .w(px(760.0))
                .flex_1()
                .min_h(px(0.0))
                .max_h(px(520.0))
                .overflow_scroll();

            for chat in &self.chats {
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
                            .px_2()
                            .py_1()
                            .rounded_full()
                            .bg(rgb(BG))
                            .text_xs()
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
                    .hover(|style| {
                        style
                            .bg(rgb(DANGER_SURFACE))
                            .border_color(rgb(DANGER))
                    })
                    .on_click(|_, _, _| {})
                    .child("Delete");

                row = row.child(details).child(delete_button);
                list = list.child(row);
            }

            body = body.child(section).child(list);

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

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(BG))
            .text_color(rgb(TEXT))
            .child(header)
            .child(body)
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

fn main() {
    let (events_tx, events_rx) = mpsc::unbounded_channel();
    whatsapp::spawn(events_tx);

    application().run(move |cx: &mut App| {
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
                let view = cx.new(|_| WhatsappCleaner::new());
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
