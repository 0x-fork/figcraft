use std::path::PathBuf;

use figcraft_engine::Session;

use crate::{DialogFuture, FigApp, Services};

struct OpenFile(PathBuf);

impl Services for OpenFile {
    fn pick_open(&mut self, _title: &str, _exts: &[&str]) -> DialogFuture {
        Box::pin(std::future::ready(Some(self.0.clone())))
    }
}

#[test]
fn open_dialog_finishes_after_the_async_picker_returns() {
    let dir = std::env::temp_dir().join(format!("figcraft-async-open-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("opened.figcraft");
    let doc = figcraft_doc::Document::new("Opened");
    std::fs::write(&path, figcraft_format::save_bytes(&doc, None).unwrap()).unwrap();

    let mut app = FigApp::new(Session::empty(), Box::new(OpenFile(path.clone())));
    app.ui.home = true;
    app.open_dialog();
    assert!(app.session.docs.is_empty(), "opening waits for the picker result");

    app.poll_dialog(&egui::Context::default());
    assert_eq!(app.session.docs.len(), 1);
    assert_eq!(app.session.docs[0].path.as_deref(), Some(path.to_string_lossy().as_ref()));
    assert!(!app.ui.home);
    let _ = std::fs::remove_dir_all(dir);
}
