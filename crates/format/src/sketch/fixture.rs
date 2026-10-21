//! Synthetic `.sketch` files for the tests: the JSON of a document, its pages and `meta.json`,
//! zipped the way Sketch writes them. Layers are built with small helpers and adjusted with
//! [`with`].

use std::io::Write;

use serde_json::{Value, json};

/// A `.sketch` zip: `document.json`, `meta.json` saying `app_version` / `version`, one
/// `pages/<id>.json` per page and any other `files` (images, fonts).
pub(crate) fn sketch_zip(app_version: &str, version: u64, document: Value, pages: &[Value], files: &[(&str, &[u8])]) -> Vec<u8> {
    let meta = json!({
        "app": "com.bohemiancoding.sketch3",
        "appVersion": app_version,
        "version": version,
        "compatibilityVersion": 99,
        "pagesAndArtboards": {},
    });
    raw_zip(&document, &meta, pages, files)
}

/// A `.sketch` zip with the given `meta.json` as is.
pub(crate) fn raw_zip(document: &Value, meta: &Value, pages: &[Value], files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut z = zip::ZipWriter::new(std::io::Cursor::new(&mut out));
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        z.start_file("document.json", opts).unwrap();
        z.write_all(document.to_string().as_bytes()).unwrap();
        z.start_file("meta.json", opts).unwrap();
        z.write_all(meta.to_string().as_bytes()).unwrap();
        for p in pages {
            let id = p["do_objectID"].as_str().unwrap_or("page");
            z.start_file(format!("pages/{id}.json"), opts).unwrap();
            z.write_all(p.to_string().as_bytes()).unwrap();
        }
        for (name, bytes) in files {
            z.start_file(*name, opts).unwrap();
            z.write_all(bytes).unwrap();
        }
        z.finish().unwrap();
    }
    out
}

/// `document.json` listing `pages` (ids) in order, with empty shared-style containers.
pub(crate) fn document(pages: &[&str]) -> Value {
    json!({
        "_class": "document",
        "do_objectID": "DOC",
        "colorSpace": 1,
        "currentPageIndex": 0,
        "foreignLayerStyles": [],
        "foreignSymbols": [],
        "foreignTextStyles": [],
        "layerStyles": {"_class": "sharedStyleContainer", "objects": []},
        "layerTextStyles": {"_class": "sharedTextStyleContainer", "objects": []},
        "layerSymbols": {"_class": "symbolContainer", "objects": []},
        "pages": pages.iter().map(|p| json!({
            "_class": "MSJSONFileReference",
            "_ref_class": "MSImmutablePage",
            "_ref": format!("pages/{p}"),
        })).collect::<Vec<_>>(),
    })
}

fn frame(x: f64, y: f64, w: f64, h: f64) -> Value {
    json!({"_class": "rect", "constrainProportions": false, "x": x, "y": y, "width": w, "height": h})
}

fn layer(class: &str, id: &str, name: &str, x: f64, y: f64, w: f64, h: f64) -> Value {
    json!({
        "_class": class,
        "do_objectID": id,
        "name": name,
        "frame": frame(x, y, w, h),
        "isVisible": true,
        "isLocked": false,
        "rotation": 0,
        "isFlippedHorizontal": false,
        "isFlippedVertical": false,
        "resizingConstraint": 63,
        "style": {"_class": "style", "fills": [], "borders": [], "shadows": [], "innerShadows": []},
    })
}

pub(crate) fn page(id: &str, name: &str, layers: Vec<Value>) -> Value {
    with(layer("page", id, name, 0.0, 0.0, 0.0, 0.0), "layers", Value::Array(layers))
}

pub(crate) fn artboard(id: &str, name: &str, x: f64, y: f64, w: f64, h: f64, layers: Vec<Value>) -> Value {
    with(layer("artboard", id, name, x, y, w, h), "layers", Value::Array(layers))
}

pub(crate) fn group(id: &str, x: f64, y: f64, w: f64, h: f64, layers: Vec<Value>) -> Value {
    with(layer("group", id, id, x, y, w, h), "layers", Value::Array(layers))
}

/// A rectangle with its four generated corner points.
pub(crate) fn rect(id: &str, x: f64, y: f64, w: f64, h: f64) -> Value {
    let pts = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)].map(|(px, py)| point(px, py));
    let mut l = with(layer("rectangle", id, id, x, y, w, h), "points", Value::Array(pts.to_vec()));
    l["isClosed"] = json!(true);
    l["fixedRadius"] = json!(0);
    l
}

/// A straight curve point at `(x, y)` in 0–1 of the layer's frame.
pub(crate) fn point(x: f64, y: f64) -> Value {
    let p = format!("{{{x}, {y}}}");
    json!({"_class": "curvePoint", "cornerRadius": 0, "curveFrom": p, "curveTo": p, "curveMode": 1, "hasCurveFrom": false, "hasCurveTo": false, "point": p})
}

/// `l` with `key` set to `value`.
pub(crate) fn with(mut l: Value, key: &str, value: Value) -> Value {
    if let Some(m) = l.as_object_mut() {
        m.insert(key.to_string(), value);
    }
    l
}

/// A `shapePath` through `points` (`(x, y)` in 0–1 of its frame), straight segments.
pub(crate) fn shape_path(id: &str, x: f64, y: f64, w: f64, h: f64, points: &[(f64, f64)], closed: bool) -> Value {
    let pts: Vec<Value> = points.iter().map(|(px, py)| point(*px, *py)).collect();
    let mut l = with(layer("shapePath", id, id, x, y, w, h), "points", Value::Array(pts));
    l["isClosed"] = json!(closed);
    l["booleanOperation"] = json!(-1);
    l
}

/// A `shapeGroup` (Sketch 51 and older wrap every shape in one) holding `layers`.
pub(crate) fn shape_group(id: &str, x: f64, y: f64, w: f64, h: f64, layers: Vec<Value>) -> Value {
    with(layer("shapeGroup", id, id, x, y, w, h), "layers", Value::Array(layers))
}
