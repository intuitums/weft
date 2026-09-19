use dioxus::prelude::*;
use wove::{Event, Key};
use wove_dioxus::{elements as dioxus_elements, View};
fn app() -> Element {
    let mut count = use_signal(|| 0);
    let mut name = use_signal(String::new);
    rsx! { view { direction:"column", onkey: move |event| {
        if let Event::Key(Key::Char('+'),_) = *event.data { count += 1; event.prevent_default(); }
        if let Event::Key(Key::Char('-'),_) = *event.data { count -= 1; event.prevent_default(); }
    },
        text { content:"Wove · Dioxus counter" }
        text { content:"Count: {count}" }
        text { content:"+ / - change · Tab focus · Esc quit" }
        input { value:"{name}", placeholder:"Type here", oninput: move |event| name.set(event.data.to_string()) }
        text { content:"Name: {name}" }
    } }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut view = View::new(VirtualDom::new(app))?;
    view.focus_next(false)?;
    let quit = |event: &Event| *event == Key::Escape.into();
    futures_lite::future::block_on(wove_dioxus::run(&mut view, quit))?;
    Ok(())
}
