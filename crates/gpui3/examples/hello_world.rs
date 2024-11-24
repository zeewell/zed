use gpui::*;
use gpui3 as gpui;

fn main() {
    App::new().run(|cx: &mut AppContext| {
        let bounds = Bounds::centered(None, size(px(300.0), px(300.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_window, _cx| {
                div()
                    .flex()
                    .bg(rgb(0x2e7d32))
                    .size(Length::Definite(Pixels(300.0).into()))
                    .justify_center()
                    .items_center()
                    .shadow_lg()
                    .border_1()
                    .border_color(rgb(0x0000ff))
                    .text_xl()
                    .text_color(rgb(0xffffff))
                    .child("Hello, World!")
            },
        )
        .unwrap();
    });
}
