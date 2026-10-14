use figcraft_engine::Session;
use serde_json::json;

use crate::{FigApp, Services};

struct Clip {
    html: String,
    text: Option<String>,
}

impl Services for Clip {
    fn clipboard_get_html(&mut self) -> Option<String> {
        Some(self.html.clone())
    }
    fn clipboard_get_text(&mut self) -> Option<String> {
        self.text.clone()
    }
}

fn figma_html() -> String {
    let mut s = Session::new();
    s.execute("node.create", &json!({"type": "rectangle", "x": 0, "y": 0, "width": 40, "height": 20, "name": "From Figma"})).unwrap();
    let r = s.execute("file.export", &json!({"format": "fig"})).unwrap();
    format!(r#"<span data-buffer="<!--(figma){}(/figma)-->"></span>"#, r["data"].as_str().unwrap())
}

fn selected_names(app: &FigApp) -> Vec<String> {
    let doc = app.session.doc().unwrap();
    app.session.selection().iter().map(|i| doc.node(*i).unwrap().name.clone()).collect()
}

#[test]
fn paste_takes_layers_copied_in_figma() {
    let html = figma_html();
    let mut app = FigApp::new(Session::new(), Box::new(Clip { html, text: Some(String::new()) }));
    crate::menus::paste(&mut app, None);
    assert_eq!(selected_names(&app), ["From Figma"]);
}

#[test]
fn html_from_other_apps_pastes_as_before() {
    let clip = Clip { html: "<b>Hello</b>".into(), text: Some("Hello".into()) };
    let mut app = FigApp::new(Session::new(), Box::new(clip));
    crate::menus::paste(&mut app, None);
    let doc = app.session.doc().unwrap();
    let id = app.session.selection()[0];
    assert_eq!(doc.node(id).unwrap().kind.text().unwrap().characters, "Hello");
}
