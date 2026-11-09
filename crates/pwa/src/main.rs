use dioxus::prelude::*;

fn main() {
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    rsx! {
        document::Link { rel: "stylesheet", href: asset!("/assets/main.css") }

        main {
            id: "app",
            h1 { "tafy" }
            p { "first-light: the light path works." }
        }
    }
}
