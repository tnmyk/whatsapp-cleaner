use gpui::{
    App, Bounds, Context, TitlebarOptions, Window, WindowBounds, WindowOptions, div, prelude::*,
    px, rgb, size,
};
use gpui_platform::application;

struct WhatsappCleaner;

impl Render for WhatsappCleaner {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_2()
            .size_full()
            .justify_center()
            .items_center()
            .bg(rgb(0x111b21))
            .text_color(rgb(0xe9edef))
            .text_xl()
            .child("WhatsApp Cleaner")
    }
}

fn main() {
    application().run(|cx: &mut App| {
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
            |_, cx| cx.new(|_| WhatsappCleaner),
        )
        .expect("failed to open window");
        cx.activate(true);
    });
}
