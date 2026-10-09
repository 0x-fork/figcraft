//! File menu: documents, save/open, import/export, versions.

use std::sync::Arc;

use base64::Engine as _;
use figcraft_doc::*;
use serde_json::{Value, json};

use super::{CommandSpec, ok};
use crate::params::*;
use crate::{DocState, EngineError, Result, Session, bad, spec};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        spec!("file.new", "New design file", "{name?}", false, |s, p| {
            let name = string(p, "name").unwrap_or_else(|| "Untitled".into());
            s.docs.push(DocState::new(Document::new(name), None));
            s.active = s.docs.len() - 1;
            Ok(json!({"index": s.active}))
        }),
        spec!("file.open", "Open", "{path} (.figcraft, .json, .svg, .png, .jpg …)", false, open),
        spec!("file.save", "Save", "{path?} (programmatic saves need a path the first time)", false, save),
        spec!("file.saveAs", "Save as", "{path}", false, save_as),
        spec!(
            "file.export",
            "Export",
            "{ids? (default selection, else the page), format? (png|jpg|svg|pdf|webp), scale? (default 1), path? (else returns base64 `data`)}",
            false,
            export
        ),
        spec!("file.exportSvg", "Export selection as SVG", "{ids?, path?}", false, |s, p| {
            let mut q = p.clone();
            if let Some(o) = q.as_object_mut() {
                o.insert("format".into(), json!("svg"));
            }
            export(s, &q)
        }),
        spec!(
            "file.exportFramesPdf",
            "Export frames to PDF",
            "{path?, page? (page id, default the current page)}  every top-level frame in reading order, one PDF page each; returns base64 `data` without a path",
            false,
            export_frames_pdf
        ),
        spec!(
            "file.import",
            "Import",
            "{path | data (base64) + name, x?, y?}  SVG becomes vectors, images become image layers, .figcraft pages are appended",
            true,
            import
        ),
        spec!("file.placeImage", "Place image", "{path | data, x?, y?}", true, super::create::place_image),
        spec!("file.close", "Close tab", "{index?, force? (discard unsaved changes)}", false, close),
        spec!("file.rename", "Rename file", "{name}", true, |s, p| {
            let name = req_string("file.rename", p, "name")?;
            s.doc_mut()?.name = name;
            ok()
        }),
        spec!("file.duplicate", "Duplicate file", "{}", false, |s, _| {
            let ds = s.ds()?;
            let mut d = (*ds.doc).clone();
            d.name = format!("{} (Copy)", d.name);
            s.docs.push(DocState::new(d, None));
            s.active = s.docs.len() - 1;
            Ok(json!({"index": s.active}))
        }),
        spec!("file.revert", "Revert to saved", "{}", false, |s, _| {
            let path = s.ds()?.path.clone().ok_or_else(|| EngineError::Disabled("file.revert".into(), "the file was never saved".into()))?;
            let (doc, _) = load(&path)?;
            let i = s.active;
            if let Some(slot) = s.docs.get_mut(i) {
                *slot = DocState::new(doc, Some(path));
            }
            ok()
        }),
        spec!("file.saveVersion", "Save to version history", "{name?, description?}", false, |s, p| {
            let name = string(p, "name").unwrap_or_else(|| format!("Version {}", s.ds().map(|d| versions(d).len() + 1).unwrap_or(1)));
            let ds = s.ds_mut()?;
            let snap = ds.doc.clone();
            push_version(ds, name, string(p, "description").unwrap_or_default(), snap);
            ok()
        }),
        spec!("file.versionHistory", "Show version history", "{}", false, |s, _| {
            let ds = s.ds()?;
            let v: Vec<Value> =
                versions(ds).iter().enumerate().map(|(i, v)| json!({"index": i, "name": v.0, "description": v.1, "time": v.2})).collect();
            s.request_ui("panel.versionHistory", json!({}));
            Ok(Value::Array(v))
        }),
        spec!("version.restore", "Restore version", "{index}", true, |s, p| {
            let i = req_num("version.restore", p, "index")? as usize;
            let ds = s.ds_mut()?;
            let Some((_, _, _, doc)) =
                version_list().lock().unwrap_or_else(std::sync::PoisonError::into_inner).get(&ds.uid).and_then(|v| v.get(i)).cloned()
            else {
                return Err(bad("version.restore", "no such version"));
            };
            ds.doc = doc;
            if !ds.doc.pages.contains(&ds.page) {
                ds.page = ds.doc.first_page().unwrap_or(ds.page);
            }
            ok()
        }),
        spec!("doc.switch", "Switch document", "{index}", false, |s, p| {
            let i = req_num("doc.switch", p, "index")? as usize;
            if i >= s.docs.len() {
                return Err(bad("doc.switch", "no such document"));
            }
            s.active = i;
            ok()
        }),
        spec!("doc.list", "List documents", "{}", false, |s, _| {
            Ok(Value::Array(
                s.docs
                    .iter()
                    .enumerate()
                    .map(|(i, d)| json!({"index": i, "name": d.doc.name, "path": d.path, "dirty": d.is_dirty(), "active": i == s.active}))
                    .collect(),
            ))
        }),
        spec!("doc.home", "Home", "{}", false, |s, _| {
            s.request_ui("home", json!({}));
            ok()
        }),
    ]
}

type Version = (String, String, u64, Arc<Document>);

fn version_list() -> &'static std::sync::Mutex<std::collections::HashMap<u64, Vec<Version>>> {
    static V: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<u64, Vec<Version>>>> = std::sync::OnceLock::new();
    V.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

fn now_secs() -> u64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
    }
    #[cfg(target_arch = "wasm32")]
    {
        0
    }
}

fn push_version(ds: &DocState, name: String, desc: String, doc: Arc<Document>) {
    version_list().lock().unwrap_or_else(std::sync::PoisonError::into_inner).entry(ds.uid).or_default().push((name, desc, now_secs(), doc));
}

pub fn versions(ds: &DocState) -> Vec<(String, String, u64)> {
    version_list()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(&ds.uid)
        .map(|v| v.iter().map(|(a, b, c, _)| (a.clone(), b.clone(), *c)).collect())
        .unwrap_or_default()
}

#[cfg(not(target_arch = "wasm32"))]
fn read(path: &str) -> Result<Vec<u8>> {
    std::fs::read(path).map_err(|e| EngineError::Other(format!("{path}: {e}")))
}

#[cfg(target_arch = "wasm32")]
fn read(path: &str) -> Result<Vec<u8>> {
    Err(EngineError::Other(format!("{path}: no file system in the browser")))
}

#[cfg(not(target_arch = "wasm32"))]
fn write(path: &str, bytes: &[u8]) -> Result<()> {
    let tmp = format!("{path}.tmp-figcraft");
    std::fs::write(&tmp, bytes).map_err(|e| EngineError::Other(format!("{tmp}: {e}")))?;
    std::fs::rename(&tmp, path).map_err(|e| EngineError::Other(format!("{path}: {e}")))
}

#[cfg(target_arch = "wasm32")]
fn write(path: &str, _bytes: &[u8]) -> Result<()> {
    Err(EngineError::Other(format!("{path}: no file system in the browser")))
}

fn ext(path: &str) -> String {
    std::path::Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase()
}

fn file_stem(path: &str) -> String {
    std::path::Path::new(path).file_stem().and_then(|e| e.to_str()).unwrap_or("Untitled").to_string()
}

/// Load a document from any supported file.
pub fn load(path: &str) -> Result<(Document, Vec<String>)> {
    let bytes = read(path)?;
    load_bytes(&bytes, path)
}

pub fn load_bytes(bytes: &[u8], name_hint: &str) -> Result<(Document, Vec<String>)> {
    match ext(name_hint).as_str() {
        "svg" => {
            let mut doc = Document::new(file_stem(name_hint));
            let page = doc.first_page().ok_or(EngineError::NoDocument)?;
            figcraft_svg::import_svg(&mut doc, bytes, page, 0.0, 0.0, &file_stem(name_hint)).map_err(|e| EngineError::Other(e.to_string()))?;
            Ok((doc, vec![]))
        }
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" => {
            let mut doc = Document::new(file_stem(name_hint));
            let page = doc.first_page().ok_or(EngineError::NoDocument)?;
            let (hash, w, h) = figcraft_format::add_image(&mut doc, bytes.to_vec()).map_err(|e| EngineError::Other(e.to_string()))?;
            let id = doc.create(page, NodeKind::Rectangle, &file_stem(name_hint), 0.0, 0.0, f64::from(w), f64::from(h))?;
            doc.get_mut(id)?.fills = vec![figcraft_color::Paint::image(hash, figcraft_color::ImageScaleMode::Fill)];
            Ok((doc, vec![]))
        }
        _ => {
            let (mut doc, issues) = figcraft_format::load_bytes(bytes).map_err(|e| EngineError::Other(e.to_string()))?;
            if doc.name.is_empty() {
                doc.name = file_stem(name_hint);
            }
            Ok((doc, issues))
        }
    }
}

fn open(s: &mut Session, p: &Value) -> Result<Value> {
    let path = req_string("file.open", p, "path")?;
    // Already open: switch to it.
    if let Some(i) = s.docs.iter().position(|d| d.path.as_deref() == Some(path.as_str())) {
        s.active = i;
        return Ok(json!({"index": i}));
    }
    let (doc, issues) = load(&path)?;
    let native = matches!(ext(&path).as_str(), "figcraft" | "json");
    s.docs.push(DocState::new(doc, native.then_some(path)));
    s.active = s.docs.len() - 1;
    Ok(json!({"index": s.active, "repairs": issues}))
}

/// Thumbnail PNG of the first page's content (for file browsers / the home screen).
pub fn thumbnail(doc: &Document) -> Option<Vec<u8>> {
    let page = doc.first_page()?;
    let b = doc.page_content_bounds(page)?;
    let scale = (400.0 / b.width().max(b.height()).max(1.0)).min(2.0);
    let view = figcraft_render::View {
        origin: (b.x0, b.y0),
        scale,
        width: (b.width() * scale).ceil().max(1.0) as u32,
        height: (b.height() * scale).ceil().max(1.0) as u32,
    };
    let pix = figcraft_render::render_view(doc, page, &view, &Default::default())?;
    figcraft_render::encode_png(&pix).ok()
}

fn save_to(s: &mut Session, path: String) -> Result<Value> {
    let ds = s.ds_mut()?;
    let mut doc = (*ds.doc).clone();
    doc.current_page = Some(ds.page);
    let thumb = thumbnail(&doc);
    let bytes = figcraft_format::save_bytes(&doc, thumb.as_deref()).map_err(|e| EngineError::Other(e.to_string()))?;
    write(&path, &bytes)?;
    ds.path = Some(path.clone());
    ds.mark_saved();
    ds.revision += 1;
    Ok(json!({"path": path, "bytes": bytes.len()}))
}

fn save(s: &mut Session, p: &Value) -> Result<Value> {
    if let Some(path) = string(p, "path") {
        return save_to(s, path);
    }
    match s.ds()?.path.clone() {
        Some(path) => save_to(s, path),
        None => {
            s.request_ui("dialog.saveAs", json!({}));
            Err(EngineError::Disabled("file.save".into(), "the file has no path yet: pass `path`".into()))
        }
    }
}

fn save_as(s: &mut Session, p: &Value) -> Result<Value> {
    match string(p, "path") {
        Some(mut path) => {
            if ext(&path).is_empty() {
                path.push_str(".figcraft");
            }
            save_to(s, path)
        }
        None => {
            s.request_ui("dialog.saveAs", json!({}));
            Err(EngineError::Disabled("file.saveAs".into(), "pass `path`".into()))
        }
    }
}

/// Encode `ids` (or the page) in `format` at `scale`.
pub fn export_bytes(doc: &Document, ids: &[NodeId], format: ExportFormat, scale: f64) -> Result<Vec<u8>> {
    if ids.is_empty() {
        return Err(EngineError::Disabled("file.export".into(), "nothing to export".into()));
    }
    match format {
        ExportFormat::Svg => figcraft_svg::export_svg(doc, ids, &Default::default())
            .map(String::into_bytes)
            .ok_or_else(|| EngineError::Other("nothing to export".into())),
        ExportFormat::Pdf => {
            // Frames export one page each; anything else shares one page.
            let all_frames = ids.iter().all(|i| doc.node(*i).is_some_and(|n| n.kind.is_frame_like()));
            let r = if all_frames {
                figcraft_pdf::export_pdf(doc, ids, &figcraft_pdf::PdfOptions::default())
            } else {
                figcraft_pdf::export_nodes_pdf(doc, &super::edit::z_sorted(doc, ids))
            };
            r.map_err(|e| EngineError::Other(e.to_string()))
        }
        _ => {
            let pix = if ids.len() == 1 {
                ids.first().and_then(|i| figcraft_render::render_node(doc, *i, scale))
            } else {
                let b = ids.iter().filter_map(|i| figcraft_render::export_bounds(doc, *i)).reduce(|a, b| a.union(b));
                b.and_then(|b| {
                    let view = figcraft_render::View {
                        origin: (b.x0, b.y0),
                        scale,
                        width: (b.width() * scale).ceil().max(1.0) as u32,
                        height: (b.height() * scale).ceil().max(1.0) as u32,
                    };
                    let mut pix = figcraft_render::Pixmap::new(view.width.clamp(1, 16384), view.height.clamp(1, 16384))?;
                    let ordered = super::edit::z_sorted(doc, ids);
                    figcraft_render::render_nodes_into(doc, &ordered, &view, &mut pix);
                    Some(pix)
                })
            };
            let pix = pix.ok_or_else(|| EngineError::Other("render failed (empty or too large)".into()))?;
            match format {
                ExportFormat::Jpg => figcraft_render::encode_jpeg(&pix, figcraft_color::Color::WHITE, 90),
                ExportFormat::Webp => figcraft_render::encode_webp(&pix),
                _ => figcraft_render::encode_png(&pix),
            }
            .map_err(EngineError::Other)
        }
    }
}

fn export(s: &mut Session, p: &Value) -> Result<Value> {
    let doc = s.doc()?;
    let mut ids = targets(s, p);
    if ids.is_empty() {
        ids = doc.get(s.page()?)?.children.clone();
    }
    let format = string(p, "format")
        .and_then(|f| ExportFormat::from_ext(&f))
        .or_else(|| string(p, "path").and_then(|pp| ExportFormat::from_ext(&ext(&pp))))
        .unwrap_or_default();
    let scale = num_or(p, "scale", 1.0).clamp(0.01, 64.0);
    let bytes = export_bytes(doc, &ids, format, scale)?;
    match string(p, "path") {
        Some(path) => {
            write(&path, &bytes)?;
            Ok(json!({"path": path, "bytes": bytes.len(), "format": format.ext()}))
        }
        None => Ok(json!({"format": format.ext(), "data": base64::engine::general_purpose::STANDARD.encode(&bytes)})),
    }
}

/// Every top-level frame of a page (reading order), one PDF page each.
fn export_frames_pdf(s: &mut Session, p: &Value) -> Result<Value> {
    let doc = s.doc()?;
    let page = match id(p, "page") {
        Some(pg) if doc.node(pg).is_some_and(Node::is_page) => pg,
        Some(_) => return Err(bad("file.exportFramesPdf", "`page` is not a page")),
        None => s.page()?,
    };
    let frames = doc.top_frames(page);
    if frames.is_empty() {
        return Err(EngineError::Disabled("file.exportFramesPdf".into(), "the page has no frames".into()));
    }
    let title = Some(doc.name.clone());
    let bytes = figcraft_pdf::export_pdf(doc, &frames, &figcraft_pdf::PdfOptions { title, ..Default::default() })
        .map_err(|e| EngineError::Other(e.to_string()))?;
    match string(p, "path") {
        Some(path) => {
            write(&path, &bytes)?;
            Ok(json!({"path": path, "bytes": bytes.len(), "pages": frames.len()}))
        }
        None => Ok(json!({"format": "pdf", "pages": frames.len(), "data": base64::engine::general_purpose::STANDARD.encode(&bytes)})),
    }
}

fn import(s: &mut Session, p: &Value) -> Result<Value> {
    let (bytes, name) = if let Some(path) = string(p, "path") {
        (read(&path)?, path)
    } else if let Some(d) = string(p, "data") {
        let d = d.split_once(',').map(|(_, b)| b.to_string()).unwrap_or(d);
        (
            base64::engine::general_purpose::STANDARD.decode(d.trim()).map_err(|e| bad("file.import", e.to_string()))?,
            string(p, "name").unwrap_or_else(|| "import.png".into()),
        )
    } else {
        return Err(bad("file.import", "pass `path` or `data`"));
    };
    let page = s.page()?;
    let (x, y) = (num(p, "x"), num(p, "y"));
    let center = s.ds().map(|d| (d.view.cx, d.view.cy)).unwrap_or((0.0, 0.0));
    match ext(&name).as_str() {
        "svg" => {
            let doc = s.doc_mut()?;
            let id = figcraft_svg::import_svg(doc, &bytes, page, x.unwrap_or(center.0), y.unwrap_or(center.1), &file_stem(&name))
                .map_err(|e| EngineError::Other(e.to_string()))?;
            s.set_selection(vec![id]);
            Ok(json!({"id": id}))
        }
        "figcraft" | "json" => {
            let (other, _) = figcraft_format::load_bytes(&bytes).map_err(|e| EngineError::Other(e.to_string()))?;
            let doc = s.doc_mut()?;
            for (k, v) in &other.images {
                doc.images.entry(k.clone()).or_insert_with(|| v.clone());
            }
            let mut new_pages = Vec::new();
            for pg in &other.pages {
                let Some(pn) = other.node(*pg) else { continue };
                let np = doc.add_page(&pn.name);
                for c in pn.children.clone() {
                    let nodes: Vec<Node> = other.subtree(c).into_iter().filter_map(|i| other.node(i).cloned()).collect();
                    let mut map = std::collections::BTreeMap::new();
                    for n in &nodes {
                        map.insert(n.id, doc.alloc_id());
                    }
                    doc.paste_nodes(&nodes, &map, c, np, None)?;
                }
                new_pages.push(np);
            }
            Ok(json!({"pages": new_pages}))
        }
        _ => {
            let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
            super::create::place_image(s, &json!({"data": b64, "x": x.unwrap_or(center.0), "y": y.unwrap_or(center.1), "name": file_stem(&name)}))
        }
    }
}

fn close(s: &mut Session, p: &Value) -> Result<Value> {
    let i = num(p, "index").map(|x| x as usize).unwrap_or(s.active);
    let Some(ds) = s.docs.get(i) else { return Err(bad("file.close", "no such document")) };
    if ds.is_dirty() && !boolean(p, "force").unwrap_or(false) {
        s.request_ui("dialog.unsaved", json!({"index": i}));
        return Err(EngineError::Disabled("file.close".into(), "the document has unsaved changes (pass force: true to discard)".into()));
    }
    s.docs.remove(i);
    if s.active >= s.docs.len() {
        s.active = s.docs.len().saturating_sub(1);
    }
    ok()
}
