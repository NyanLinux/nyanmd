use crate::vim::{Act, Ed, Mode};
use pulldown_cmark::{html, Options, Parser};
use serde::de::DeserializeOwned;
use serde::Serialize;
use sycamore::futures::spawn_local_scoped;
use sycamore::prelude::*;
use sycamore::web::events::{KeyboardEvent, MouseEvent};
use sycamore::web::js_sys;
use wasm_bindgen::prelude::*;
use web_sys::{Element, HtmlElement, HtmlTextAreaElement};

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "core"], catch)]
    async fn invoke(cmd: &str, args: JsValue) -> Result<JsValue, JsValue>;
}

async fn call<T: DeserializeOwned>(cmd: &str, args: impl Serialize) -> Result<T, String> {
    let args = serde_wasm_bindgen::to_value(&args).map_err(|e| e.to_string())?;
    let v = invoke(cmd, args)
        .await
        .map_err(|e| e.as_string().unwrap_or_else(|| format!("{e:?}")))?;
    serde_wasm_bindgen::from_value(v).map_err(|e| e.to_string())
}

#[derive(Serialize)]
struct PathArg<'a> {
    path: &'a str,
}

#[derive(Serialize)]
struct WriteArg<'a> {
    path: &'a str,
    content: &'a str,
}

const NOTE_SCHEME: &str = "nyan:";

/// `[[note]]` / `[[note|alias]]` -> `[alias](nyan:note)` so the markdown parser does the rest.
fn expand_wikilinks(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut rest = src;
    while let Some(start) = rest.find("[[") {
        let after = &rest[start + 2..];
        match after.find("]]") {
            Some(end) if !after[..end].contains('\n') && !after[..end].is_empty() => {
                let inner = &after[..end];
                let (target, label) = inner.split_once('|').unwrap_or((inner, inner));
                out.push_str(&rest[..start]);
                out.push_str(&format!("[{}]({NOTE_SCHEME}{})", label.trim(), target.trim()));
                rest = &after[end + 2..];
            }
            _ => {
                out.push_str(&rest[..start + 2]);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

fn md_to_html(src: &str) -> String {
    let opts = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let mut out = String::new();
    html::push_html(&mut out, Parser::new_ext(&expand_wikilinks(src), opts));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wikilinks() {
        assert_eq!(expand_wikilinks("see [[todo]] and [[a/b|B]]"), "see [todo](nyan:todo) and [B](nyan:a/b)");
        assert_eq!(expand_wikilinks("[[open\nline]] [[]] [[x"), "[[open\nline]] [[]] [[x");
        assert!(md_to_html("[[ жагсаалт ]]").contains(r#"href="nyan:%D0%B6"#));
    }
}

#[component]
pub fn App() -> View {
    let notes = create_signal(Vec::<String>::new());
    let backlinks = create_signal(Vec::<String>::new());
    let path = create_signal(String::new());
    let content = create_signal(String::new());
    let mode = create_signal(Mode::Normal);
    let status = create_signal(String::from("nyanmd — :e name to open or create a note"));
    let cmd = create_signal(String::new());
    let ed = create_signal(Ed::new());
    let ta_ref = create_node_ref();
    let cmd_ref = create_node_ref();
    let preview_ref = create_node_ref();

    // dangerously_set_inner_html is static in sycamore 0.9, so drive it from an effect.
    create_effect(move || {
        let html = md_to_html(&content.get_clone());
        if let Some(n) = preview_ref.try_get() {
            n.unchecked_into::<HtmlElement>().set_inner_html(&html);
        }
    });
    let mode_label = create_memo(move || match mode.get() {
        Mode::Normal => "NORMAL",
        Mode::Insert => "INSERT",
        Mode::Command => "COMMAND",
    });
    let cmd_class = create_memo(move || {
        if mode.get() == Mode::Command { "cmdline" } else { "cmdline hidden" }
    });

    let textarea = move || ta_ref.get().unchecked_into::<HtmlTextAreaElement>();
    let focus_ta = move || textarea().focus().ok();

    // Push editor state into the textarea; a 1-char selection is the block cursor.
    let render = move || {
        ed.with(|e| {
            let t = textarea();
            let s = e.string();
            if t.value() != s {
                t.set_value(&s);
            }
            let c = e.cur as u32;
            let end = if e.mode == Mode::Insert { c } else { c + 1 };
            t.set_selection_range(c, end).ok();
            content.set(s);
            mode.set(e.mode);
        });
    };

    let refresh = move || {
        spawn_local_scoped(async move {
            match call::<Vec<String>>("list_notes", ()).await {
                Ok(v) => notes.set(v),
                Err(e) => status.set(e),
            }
            let p = path.get_clone();
            if p.is_empty() {
                backlinks.set(Vec::new());
                return;
            }
            match call::<Vec<String>>("backlinks", PathArg { path: &p }).await {
                Ok(v) => backlinks.set(v),
                Err(e) => status.set(e),
            }
        })
    };

    let open = move |p: String| {
        let p = if p.ends_with(".md") { p } else { format!("{p}.md") };
        spawn_local_scoped(async move {
            match call::<String>("read_note", PathArg { path: &p }).await {
                Ok(s) => {
                    ed.update(|e| e.load(&s));
                    status.set(format!("\"{p}\" {} chars", s.chars().count()));
                    path.set(p);
                    render();
                    focus_ta();
                    refresh();
                }
                Err(e) => status.set(e),
            }
        })
    };

    // Capture path/content now: `:wq` clears them before the future runs.
    let save = move || {
        let p = path.get_clone();
        let c = content.get_clone();
        if p.is_empty() {
            status.set("E32: No file name (use :e name)".into());
            return;
        }
        spawn_local_scoped(async move {
            match call::<()>("write_note", WriteArg { path: &p, content: &c }).await {
                Ok(()) => {
                    status.set(format!("\"{p}\" written"));
                    refresh();
                }
                Err(e) => status.set(e),
            }
        })
    };

    let close = move || {
        ed.update(|e| e.load(""));
        path.set(String::new());
        backlinks.set(Vec::new());
        render();
    };

    on_mount(move || {
        textarea().set_spellcheck(false);
        refresh();
        focus_ta();
    });

    let on_key = move |e: KeyboardEvent| {
        if e.meta_key() || e.ctrl_key() || e.alt_key() {
            return;
        }
        let k = e.key();
        match mode.get() {
            Mode::Insert => {
                if k == "Escape" {
                    e.prevent_default();
                    let t = textarea();
                    let caret = t.selection_start().ok().flatten().unwrap_or(0) as usize;
                    ed.update(|ed| ed.leave_insert(&t.value(), caret));
                    render();
                }
            }
            Mode::Normal => {
                e.prevent_default();
                match ed.update(|ed| ed.key(&k)) {
                    Act::Command => {
                        mode.set(Mode::Command);
                        cmd.set(String::new());
                        cmd_ref.get().unchecked_into::<HtmlElement>().focus().ok();
                    }
                    _ => render(),
                }
            }
            Mode::Command => {}
        }
    };

    // Clicking moves the block cursor to the click point.
    let on_mouseup = move |_: MouseEvent| {
        if mode.get() == Mode::Normal {
            let caret = textarea().selection_start().ok().flatten().unwrap_or(0) as usize;
            ed.update(|ed| {
                ed.leave_insert(&textarea().value(), caret + 1);
                ed.clamp();
            });
            render();
        }
    };

    let on_input = move |_| content.set(textarea().value());

    // Clicking a [[wikilink]] in the preview opens that note.
    let on_preview_click = move |e: MouseEvent| {
        let Some(a) = e
            .target()
            .and_then(|t| t.dyn_into::<Element>().ok())
            .and_then(|el| el.closest("a").ok().flatten())
        else {
            return;
        };
        let Some(name) = a.get_attribute("href").and_then(|h| h.strip_prefix(NOTE_SCHEME).map(str::to_string)) else {
            return;
        };
        e.prevent_default();
        let name = js_sys::decode_uri_component(&name).map(|s| String::from(s)).unwrap_or(name);
        open(name);
    };

    let on_cmd_key = move |e: KeyboardEvent| {
        let k = e.key();
        if k != "Enter" && k != "Escape" {
            return;
        }
        e.prevent_default();
        let act = if k == "Enter" {
            ed.update(|ed| ed.exec(&cmd.get_clone()))
        } else {
            ed.update(|ed| ed.mode = Mode::Normal);
            Act::None
        };
        render();
        focus_ta();
        match act {
            Act::Write => save(),
            Act::WriteQuit => {
                save();
                close();
            }
            Act::Quit => close(),
            Act::Edit(name) => open(name),
            Act::Unknown(c) => status.set(format!("E492: Not an editor command: {c}")),
            _ => {}
        }
    };

    view! {
        div(class="app") {
            aside(class="sidebar") {
                div(class="sidebar-title") { "~/nyanmd" }
                ul {
                    Indexed(list=notes, view=move |n| {
                        let name = n.clone();
                        let cls = move || if path.get_clone() == name { "active" } else { "" };
                        let n2 = n.clone();
                        view! { li(class=cls, on:click=move |_| open(n2.clone())) { (n) } }
                    })
                }
            }
            textarea(r#ref=ta_ref, class="editor",
                on:keydown=on_key, on:input=on_input, on:mouseup=on_mouseup,
                placeholder=":e name  to open a note")
            div(class="right") {
                section(r#ref=preview_ref, class="preview", on:click=on_preview_click)
                div(class="backlinks") {
                    div(class="backlinks-title") {
                        (move || format!("Linked mentions ({})", backlinks.with(Vec::len)))
                    }
                    ul {
                        Indexed(list=backlinks, view=move |n| {
                            let n2 = n.clone();
                            view! { li(on:click=move |_| open(n2.clone())) { (n) } }
                        })
                    }
                }
            }
        }
        footer(class="statusline") {
            span(class="mode") { (mode_label) }
            span(class="file") { (move || { let p = path.get_clone(); if p.is_empty() { "[No Name]".into() } else { p } }) }
            span(class="msg") { (status) }
            input(r#ref=cmd_ref, class=cmd_class, bind:value=cmd, on:keydown=on_cmd_key)
        }
    }
}
