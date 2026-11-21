mod whatsapp;

use std::sync::Arc;

use gpui::{
    App, Bounds, Context, ImageSource, RenderImage, SharedString, TitlebarOptions, Window,
    WindowBounds, WindowOptions, div, img, prelude::*, px, rgb, size,
};
use gpui_platform::application;
use tokio::sync::mpsc;
use whatsapp::SessionEvent;

struct WhatsappCleaner {
    qr: Option<Arc<RenderImage>>,
    status: SharedString,
    hint: SharedString,
}

impl WhatsappCleaner {
    fn new() -> Self {
        Self {
            qr: None,
            status: "Connecting to WhatsApp…".into(),
            hint: "".into(),
        }
    }

    fn handle(&mut self, event: SessionEvent) {
        match event {
            SessionEvent::Qr(code) => {
                self.qr = qr_render_image(&code).map(Arc::new);
                self.status = "Scan to link this device".into();
                self.hint = "WhatsApp → Settings → Linked devices → Link a device".into();
            }
            SessionEvent::Connected => {
                self.qr = None;
                self.status = "Connected".into();
                self.hint = "".into();
            }
            SessionEvent::LoggedOut => {
                self.qr = None;
                self.status = "Logged out".into();
                self.hint = "Restart the app to link a device again".into();
            }
            SessionEvent::Error(error) => {
                self.qr = None;
                self.status = "WhatsApp error".into();
                self.hint = error.into();
            }
        }
    }
}

impl Render for WhatsappCleaner {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let mut panel = div()
            .flex()
            .flex_col()
            .items_center()
            .gap_4()
            .child(div().text_2xl().child("WhatsApp Cleaner"));

        if let Some(qr) = &self.qr {
            panel = panel.child(
                div()
                    .p_4()
                    .bg(rgb(0xffffff))
                    .rounded_xl()
                    .child(img(ImageSource::Render(qr.clone())).size(px(360.0))),
            );
        }

        panel = panel.child(div().text_lg().child(self.status.clone()));

        if !self.hint.is_empty() {
            panel = panel.child(
                div()
                    .text_sm()
                    .text_color(rgb(0x8696a0))
                    .child(self.hint.clone()),
            );
        }

        div()
            .size_full()
            .flex()
            .justify_center()
            .items_center()
            .bg(rgb(0x111b21))
            .text_color(rgb(0xe9edef))
            .child(panel)
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
        let bounds = Bounds::centered(None, size(px(900.0), px(600.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
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
