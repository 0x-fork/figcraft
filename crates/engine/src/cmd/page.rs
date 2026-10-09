//! Pages.

use figcraft_doc::*;
use serde_json::{Value, json};

use super::{CommandSpec, ok};
use crate::params::*;
use crate::{bad, spec};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        spec!("page.add", "Add page", "{name?}", true, |s, p| {
            let doc = s.doc_mut()?;
            let name = string(p, "name").unwrap_or_else(|| format!("Page {}", doc.pages.len() + 1));
            let id = doc.add_page(&name);
            let ds = s.ds_mut()?;
            ds.page = id;
            ds.selection.clear();
            ds.zoom_to_fit();
            Ok(json!({"id": id}))
        }),
        spec!("page.select", "Go to page", "{id | index}", false, |s, p| {
            let doc = s.doc()?;
            let id = match id(p, "id") {
                Some(i) => i,
                None => {
                    let k = req_num("page.select", p, "index")? as usize;
                    *doc.pages.get(k).ok_or_else(|| bad("page.select", "no such page"))?
                }
            };
            if !doc.pages.contains(&id) {
                return Err(bad("page.select", format!("{id} is not a page")));
            }
            let ds = s.ds_mut()?;
            ds.page = id;
            ds.selection.clear();
            ds.text_edit = None;
            ds.zoom_to_fit();
            ok()
        }),
        spec!("page.rename", "Rename page", "{id?, name}", true, |s, p| {
            let name = req_string("page.rename", p, "name")?;
            let id = id(p, "id").unwrap_or(s.page()?);
            s.doc_mut()?.get_mut(id)?.name = name;
            ok()
        }),
        spec!("page.delete", "Delete page", "{id?}", true, |s, p| {
            let id = id(p, "id").unwrap_or(s.page()?);
            let doc = s.doc_mut()?;
            doc.remove(id)?;
            let first = doc.first_page();
            let ds = s.ds_mut()?;
            if ds.page == id
                && let Some(f) = first
            {
                ds.page = f;
                ds.selection.clear();
            }
            ok()
        }),
        spec!("page.duplicate", "Duplicate page", "{id?}", true, |s, p| {
            let src = id(p, "id").unwrap_or(s.page()?);
            let doc = s.doc_mut()?;
            let name = format!("{} (Copy)", doc.get(src)?.name);
            let np = doc.add_page(&name);
            if let NodeKind::Page { background, .. } = doc.get(src)?.kind.clone()
                && let NodeKind::Page { background: b, .. } = &mut doc.get_mut(np)?.kind
            {
                *b = background;
            }
            for c in doc.get(src)?.children.clone() {
                doc.duplicate_into(c, np, None)?;
            }
            // Keep the new page right after the source.
            if let Some(si) = doc.pages.iter().position(|x| *x == src) {
                doc.pages.retain(|x| *x != np);
                doc.pages.insert(si + 1, np);
            }
            s.ds_mut()?.page = np;
            Ok(json!({"id": np}))
        }),
        spec!("page.move", "Move page", "{id?, index}", true, |s, p| {
            let id = id(p, "id").unwrap_or(s.page()?);
            let k = req_num("page.move", p, "index")?.max(0.0) as usize;
            let doc = s.doc_mut()?;
            doc.pages.retain(|x| *x != id);
            let k = k.min(doc.pages.len());
            doc.pages.insert(k, id);
            ok()
        }),
        spec!("page.setBackground", "Page background", "{id?, color}", true, |s, p| {
            let c = color(p, "color").ok_or_else(|| bad("page.setBackground", "`color` required"))?;
            let id = id(p, "id").unwrap_or(s.page()?);
            if let NodeKind::Page { background, .. } = &mut s.doc_mut()?.get_mut(id)?.kind {
                *background = c;
            }
            ok()
        }),
        spec!("page.list", "List pages", "{}", false, |s, _| {
            let doc = s.doc()?;
            let cur = s.page()?;
            Ok(Value::Array(doc.pages.iter().filter_map(|p| doc.node(*p)).map(|n| json!({"id": n.id, "name": n.name, "current": n.id == cur, "children": n.children.len()})).collect()))
        }),
        spec!("guide.add", "Add guide", "{vertical (bool), offset, page?}", true, |s, p| {
            let id = id(p, "page").unwrap_or(s.page()?);
            let g = Guide { vertical: boolean(p, "vertical").unwrap_or(true), offset: req_num("guide.add", p, "offset")? };
            if let NodeKind::Page { guides, .. } = &mut s.doc_mut()?.get_mut(id)?.kind {
                guides.push(g);
            }
            ok()
        }),
        spec!("guide.remove", "Remove guide", "{index, page?}", true, |s, p| {
            let id = id(p, "page").unwrap_or(s.page()?);
            let k = req_num("guide.remove", p, "index")? as usize;
            if let NodeKind::Page { guides, .. } = &mut s.doc_mut()?.get_mut(id)?.kind
                && k < guides.len()
            {
                guides.remove(k);
            }
            ok()
        }),
    ]
}
