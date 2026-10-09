//! On-canvas text editing: caret, selection, typing. A whole editing session is one undo step.

use figcraft_doc::{NodeId, NodeKind, TextData};

use crate::{EngineError, Result, Session, TextEdit};

fn text_of(s: &Session, id: NodeId) -> Result<TextData> {
    match &s.doc()?.get(id)?.kind {
        NodeKind::Text(t) => Ok(t.clone()),
        _ => Err(EngineError::Other(format!("{id} is not a text layer"))),
    }
}

/// Start editing text node `id` with the caret at `caret` (None = select all).
pub fn begin(s: &mut Session, id: NodeId, caret: Option<usize>, created: bool) -> Result<()> {
    let t = text_of(s, id)?;
    let n = t.char_count();
    s.begin_interaction("Edit text")?;
    let ds = s.ds_mut()?;
    let (caret, anchor) = match caret {
        Some(c) => (c.min(n), c.min(n)),
        None => (n, 0),
    };
    ds.text_edit = Some(TextEdit { id, caret, anchor, created });
    ds.selection = vec![id];
    ds.revision += 1;
    Ok(())
}

/// Finish editing: an empty new text layer is removed; the session becomes one undo step.
pub fn end(s: &mut Session) -> Result<()> {
    let Some(te) = s.ds()?.text_edit.clone() else { return Ok(()) };
    s.ds_mut()?.text_edit = None;
    let empty = text_of(s, te.id).map(|t| t.characters.trim().is_empty()).unwrap_or(false);
    if empty {
        let doc = s.doc_mut()?;
        let _ = doc.remove(te.id);
        s.ds_mut()?.selection.clear();
    }
    s.commit_interaction()?;
    if empty && te.created {
        // Nothing to undo for a text box that was never filled.
        let ds = s.ds_mut()?;
        if let Some(last) = ds.history.undo.last()
            && *last.doc == *ds.doc
        {
            ds.history.undo.pop();
        }
    }
    let ds = s.ds_mut()?;
    ds.revision += 1;
    Ok(())
}

fn range(te: &TextEdit) -> (usize, usize) {
    (te.caret.min(te.anchor), te.caret.max(te.anchor))
}

fn edit(s: &mut Session, f: impl FnOnce(&mut TextData, &mut TextEdit)) -> Result<()> {
    let ds = s.ds_mut()?;
    let Some(mut te) = ds.text_edit.clone() else { return Err(EngineError::Other("not editing text".into())) };
    let before = ds.doc.clone();
    let doc = ds.doc_mut();
    let n = doc.get_mut(te.id)?;
    let NodeKind::Text(t) = &mut n.kind else { return Err(EngineError::Other("not a text layer".into())) };
    f(t, &mut te);
    let len = t.char_count();
    te.caret = te.caret.min(len);
    te.anchor = te.anchor.min(len);
    if n.source.is_some() {
        n.overrides.insert("characters".into());
    }
    crate::normalize(doc, &before);
    ds.text_edit = Some(te);
    ds.revision += 1;
    Ok(())
}

/// Type (or paste) `text` replacing the selection. Line endings are normalised and other control
/// characters dropped, so pasted plain text from any platform lands cleanly.
pub fn insert(s: &mut Session, text: &str) -> Result<()> {
    let text: String = text
        .replace("\r\n", "\n")
        .replace(['\r', '\u{2028}', '\u{2029}'], "\n")
        .chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .collect();
    if text.is_empty() {
        return Ok(());
    }
    edit(s, |t, te| {
        let (a, b) = range(te);
        t.replace_range(a, b, &text);
        te.caret = a + text.chars().count();
        te.anchor = te.caret;
    })
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn word_left(chars: &[char], mut i: usize) -> usize {
    while i > 0 && chars.get(i - 1).is_some_and(|c| !is_word(*c)) {
        i -= 1;
    }
    while i > 0 && chars.get(i - 1).is_some_and(|c| is_word(*c)) {
        i -= 1;
    }
    i
}

fn word_right(chars: &[char], mut i: usize) -> usize {
    let n = chars.len();
    while i < n && chars.get(i).is_some_and(|c| !is_word(*c)) {
        i += 1;
    }
    while i < n && chars.get(i).is_some_and(|c| is_word(*c)) {
        i += 1;
    }
    i
}

/// A named key: Backspace, Delete, Left, Right, Up, Down, Home, End, Enter, Escape, SelectAll.
/// `word` is Alt (word-wise), `line` is Cmd (line-wise).
pub fn key(s: &mut Session, key: &str, shift: bool, word: bool, line: bool) -> Result<()> {
    let Some(te) = s.ds()?.text_edit.clone() else { return Err(EngineError::Other("not editing text".into())) };
    let t = text_of(s, te.id)?;
    let chars: Vec<char> = t.characters.chars().collect();
    let n = chars.len();
    match key {
        "Escape" => return end(s),
        "Enter" => return insert(s, "\n"),
        "Tab" => return insert(s, "\t"),
        "Backspace" | "Delete" => {
            return edit(s, |t, te| {
                let (a, b) = range(te);
                let (a, b) = if a != b {
                    (a, b)
                } else if key == "Backspace" {
                    let from = if line {
                        line_start(&chars, a)
                    } else if word {
                        word_left(&chars, a)
                    } else {
                        a.saturating_sub(1)
                    };
                    (from, a)
                } else {
                    let to = if line {
                        line_end(&chars, a)
                    } else if word {
                        word_right(&chars, a)
                    } else {
                        (a + 1).min(n)
                    };
                    (a, to)
                };
                t.replace_range(a, b, "");
                te.caret = a;
                te.anchor = a;
            });
        }
        _ => {}
    }
    // Caret movement.
    let layout = {
        let doc = s.doc()?;
        let node = doc.get(te.id)?;
        figcraft_text::layout_node(node).map(|(l, _)| l)
    };
    let (a, b) = range(&te);
    // Without Shift, a selection collapses first: plain arrows stop at its edge, word/line moves
    // continue from that edge.
    let cur = match key {
        "Left" | "Up" | "Home" if !shift && a != b => a,
        "Right" | "Down" | "End" if !shift && a != b => b,
        _ => te.caret,
    };
    let next = match key {
        "Left" if !shift && a != b && !word && !line => a,
        "Right" if !shift && a != b && !word && !line => b,
        "Left" if line => line_start(&chars, cur),
        "Right" if line => line_end(&chars, cur),
        "Left" if word => word_left(&chars, cur),
        "Right" if word => word_right(&chars, cur),
        "Left" => cur.saturating_sub(1),
        "Right" => (cur + 1).min(n),
        "Home" => line_start(&chars, cur),
        "End" => line_end(&chars, cur),
        "Up" | "Down" => match &layout {
            _ if line => {
                if key == "Up" {
                    0
                } else {
                    n
                }
            }
            Some(l) => {
                let (x, li) = l.caret(cur);
                let target = if key == "Up" { li.checked_sub(1) } else { Some(li + 1).filter(|k| *k < l.lines.len()) };
                match target.and_then(|k| l.lines.get(k)) {
                    Some(line) => l.hit(x, line.top + line.height / 2.0),
                    None => {
                        if key == "Up" {
                            0
                        } else {
                            n
                        }
                    }
                }
            }
            None => cur,
        },
        "SelectAll" => {
            let ds = s.ds_mut()?;
            if let Some(te) = ds.text_edit.as_mut() {
                te.anchor = 0;
                te.caret = n;
            }
            ds.revision += 1;
            return Ok(());
        }
        _ => return Ok(()),
    };
    let ds = s.ds_mut()?;
    if let Some(te) = ds.text_edit.as_mut() {
        te.caret = next;
        if !shift {
            te.anchor = next;
        }
    }
    ds.revision += 1;
    Ok(())
}

fn line_start(chars: &[char], mut i: usize) -> usize {
    while i > 0 && chars.get(i - 1).is_some_and(|c| *c != '\n') {
        i -= 1;
    }
    i
}

fn line_end(chars: &[char], mut i: usize) -> usize {
    while i < chars.len() && chars.get(i).is_some_and(|c| *c != '\n') {
        i += 1;
    }
    i
}

/// Place the caret (and optionally extend the selection) at char `index`.
pub fn set_caret(s: &mut Session, index: usize, extend: bool) -> Result<()> {
    let len = match s.ds()?.text_edit.as_ref() {
        Some(te) => text_of(s, te.id).map(|t| t.char_count()).unwrap_or(0),
        None => 0,
    };
    let index = index.min(len);
    let ds = s.ds_mut()?;
    if let Some(te) = ds.text_edit.as_mut() {
        te.caret = index;
        if !extend {
            te.anchor = index;
        }
    }
    ds.revision += 1;
    Ok(())
}

/// Select the word at `index` (double-click).
pub fn select_word(s: &mut Session, index: usize) -> Result<()> {
    let Some(te) = s.ds()?.text_edit.clone() else { return Ok(()) };
    let chars: Vec<char> = text_of(s, te.id)?.characters.chars().collect();
    let mut a = index.min(chars.len());
    // Double-clicking just after a word's last char (or at the end) selects that word.
    if a > 0 && chars.get(a).is_none_or(|c| !is_word(*c)) && chars.get(a - 1).is_some_and(|c| is_word(*c)) {
        a -= 1;
    }
    let mut b = a;
    let class = |c: char| {
        if is_word(c) {
            0
        } else if c.is_whitespace() {
            1
        } else {
            2
        }
    };
    // A word, a run of spaces or a run of punctuation, like the incumbent; newlines stay unselected.
    if let Some(k) = chars.get(a).filter(|c| **c != '\n').map(|c| class(*c)) {
        while a > 0 && chars.get(a - 1).is_some_and(|c| *c != '\n' && class(*c) == k) {
            a -= 1;
        }
        while b < chars.len() && chars.get(b).is_some_and(|c| *c != '\n' && class(*c) == k) {
            b += 1;
        }
    }
    let ds = s.ds_mut()?;
    if let Some(te) = ds.text_edit.as_mut() {
        te.anchor = a;
        te.caret = b;
    }
    ds.revision += 1;
    Ok(())
}

/// Select the line (paragraph) containing `index` (triple-click).
pub fn select_line(s: &mut Session, index: usize) -> Result<()> {
    let Some(te) = s.ds()?.text_edit.clone() else { return Ok(()) };
    let chars: Vec<char> = text_of(s, te.id)?.characters.chars().collect();
    let i = index.min(chars.len());
    let (a, b) = (line_start(&chars, i), line_end(&chars, i));
    let ds = s.ds_mut()?;
    if let Some(te) = ds.text_edit.as_mut() {
        te.anchor = a;
        te.caret = b;
    }
    ds.revision += 1;
    Ok(())
}

/// The selected text while editing (for copy).
pub fn selected_text(s: &Session) -> Option<String> {
    let (id, a, b) = selected_range(s)?;
    let doc = s.doc().ok()?;
    let t = doc.node(id)?.kind.text()?;
    Some(t.characters.chars().skip(a).take(b.saturating_sub(a)).collect())
}

/// The selected range while editing (for styling commands), or None.
pub fn selected_range(s: &Session) -> Option<(NodeId, usize, usize)> {
    let te = s.ds().ok()?.text_edit.clone()?;
    let (a, b) = range(&te);
    Some((te.id, a, b))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::{PointerEvent, PointerKind};
    use figcraft_doc::{ListStyle, TextAutoResize, TextCase, TextDecoration};
    use serde_json::json;

    fn setup(text: &str) -> (Session, NodeId) {
        let mut s = Session::new();
        let r = s.execute("text.create", &json!({"text": text, "x": 0, "y": 0, "props": {"fontSize": 24}})).unwrap();
        let id = crate::params::id_of(&r["id"]).unwrap();
        (s, id)
    }

    fn data(s: &Session, id: NodeId) -> TextData {
        s.doc().unwrap().node(id).unwrap().kind.text().unwrap().clone()
    }

    fn sel(s: &Session) -> (usize, usize) {
        let te = s.ds().unwrap().text_edit.clone().unwrap();
        (te.anchor, te.caret)
    }

    #[test]
    fn whole_editing_session_is_one_undo_step() {
        let (mut s, id) = setup("Hello");
        let undo_before = s.ds().unwrap().history.undo.len();
        begin(&mut s, id, Some(5), false).unwrap();
        insert(&mut s, " big").unwrap();
        insert(&mut s, " world").unwrap();
        key(&mut s, "Backspace", false, false, false).unwrap();
        // Styling a range mid-session joins the same step.
        set_caret(&mut s, 0, false).unwrap();
        set_caret(&mut s, 5, true).unwrap();
        s.execute("text.bold", &json!({})).unwrap();
        s.execute("text.alignCenter", &json!({})).unwrap();
        end(&mut s).unwrap();
        let t = data(&s, id);
        assert_eq!(t.characters, "Hello big worl");
        assert_eq!(t.style_at(0).font_style, "Bold");
        assert_eq!(t.style_at(6).font_style, "Regular");
        assert_eq!(s.ds().unwrap().history.undo.len(), undo_before + 1);
        s.undo().unwrap();
        let t = data(&s, id);
        assert_eq!(t.characters, "Hello");
        assert!(t.runs.is_empty());
        assert_eq!(t.align_h, figcraft_doc::TextAlignH::Left);
    }

    #[test]
    fn typing_replaces_selection_and_paste_is_plain() {
        let (mut s, id) = setup("Hello world");
        begin(&mut s, id, None, false).unwrap();
        assert_eq!(sel(&s), (0, 11), "begin without caret selects all");
        select_word(&mut s, 7).unwrap();
        assert_eq!(sel(&s), (6, 11));
        insert(&mut s, "there").unwrap();
        assert_eq!(data(&s, id).characters, "Hello there");
        assert_eq!(sel(&s), (11, 11));
        // Pasted text: CRLF / CR / Unicode separators become newlines, other controls are dropped.
        insert(&mut s, "\r\na\rb\u{2029}c\u{0}\u{7}\td").unwrap();
        assert_eq!(data(&s, id).characters, "Hello there\na\nb\nc\td");
        insert(&mut s, "\u{1}").unwrap();
        assert_eq!(data(&s, id).characters, "Hello there\na\nb\nc\td");
        // Select the line (paragraph) then type over it.
        select_line(&mut s, 0).unwrap();
        assert_eq!(sel(&s), (0, 11));
        insert(&mut s, "Hi").unwrap();
        assert_eq!(data(&s, id).characters, "Hi\na\nb\nc\td");
        assert_eq!(selected_text(&s).as_deref(), Some(""));
        end(&mut s).unwrap();
    }

    #[test]
    fn word_selection_classes() {
        let (mut s, id) = setup("foo,  bar_baz!\nnext");
        begin(&mut s, id, Some(0), false).unwrap();
        select_word(&mut s, 1).unwrap();
        assert_eq!(sel(&s), (0, 3));
        // Right after a word's last char selects the word.
        select_word(&mut s, 13).unwrap();
        assert_eq!(sel(&s), (6, 13));
        // A run of spaces.
        select_word(&mut s, 4).unwrap();
        assert_eq!(sel(&s), (4, 6));
        select_line(&mut s, 16).unwrap();
        assert_eq!(sel(&s), (15, 19));
        select_line(&mut s, 999).unwrap();
        assert_eq!(sel(&s), (15, 19));
        select_word(&mut s, 999).unwrap();
        assert_eq!(sel(&s), (15, 19));
        set_caret(&mut s, 999, false).unwrap();
        assert_eq!(sel(&s), (19, 19), "carets clamp to the text");
    }

    #[test]
    fn keyboard_navigation_and_extension() {
        let (mut s, id) = setup("one two\nthree");
        begin(&mut s, id, Some(0), false).unwrap();
        key(&mut s, "Right", false, true, false).unwrap(); // Alt+Right: end of word
        assert_eq!(sel(&s), (3, 3));
        key(&mut s, "Right", true, true, false).unwrap(); // Shift+Alt+Right extends
        assert_eq!(sel(&s), (3, 7));
        key(&mut s, "Left", false, false, false).unwrap(); // collapses to the start
        assert_eq!(sel(&s), (3, 3));
        key(&mut s, "Right", false, false, true).unwrap(); // Cmd+Right: line end
        assert_eq!(sel(&s), (7, 7));
        key(&mut s, "Down", true, false, true).unwrap(); // Shift+Cmd+Down: to the end
        assert_eq!(sel(&s), (7, 13));
        key(&mut s, "Left", false, false, true).unwrap(); // Cmd+Left from the collapsed start
        assert_eq!(sel(&s), (0, 0));
        key(&mut s, "Delete", false, true, false).unwrap(); // Alt+Delete: word forward
        assert_eq!(data(&s, id).characters, " two\nthree");
        key(&mut s, "SelectAll", false, false, false).unwrap();
        assert_eq!(sel(&s), (0, 10));
        end(&mut s).unwrap();
    }

    fn ev(kind: PointerKind, x: f64, y: f64, shift: bool) -> PointerEvent {
        let mut e = PointerEvent::at(kind, x, y, 1.0);
        e.shift = shift;
        e
    }

    #[test]
    fn pointer_double_click_drag_and_shift_click() {
        let (mut s, id) = setup("Hello world");
        let Some((l, _)) = figcraft_text::layout_node(s.doc().unwrap().node(id).unwrap()) else { return };
        if l.glyphs.is_empty() {
            return; // no fonts
        }
        let y = l.lines[0].top + l.lines[0].height / 2.0;
        let x_of = |i: usize| l.caret(i).0;
        // Double-click inside "world" edits and selects the word.
        let mid = (x_of(7) + x_of(8)) / 2.0;
        s.pointer(&ev(PointerKind::Down, mid, y, false)).unwrap();
        s.pointer(&ev(PointerKind::Up, mid, y, false)).unwrap();
        s.pointer(&ev(PointerKind::DoubleClick, mid, y, false)).unwrap();
        assert_eq!(sel(&s), (6, 11));
        // Drag-select from the start of "Hello" to the middle.
        s.pointer(&ev(PointerKind::Down, x_of(0) + 0.5, y, false)).unwrap();
        assert_eq!(sel(&s), (0, 0));
        s.pointer(&ev(PointerKind::Move, (x_of(3) + x_of(4)) / 2.0 - 1.0, y, false)).unwrap();
        s.pointer(&ev(PointerKind::Up, (x_of(3) + x_of(4)) / 2.0 - 1.0, y, false)).unwrap();
        assert_eq!(sel(&s), (0, 3));
        // Shift+click extends.
        s.pointer(&ev(PointerKind::Down, x_of(9) + 0.5, y, true)).unwrap();
        s.pointer(&ev(PointerKind::Up, x_of(9) + 0.5, y, true)).unwrap();
        assert_eq!(sel(&s), (0, 9));
        insert(&mut s, "J").unwrap();
        assert_eq!(data(&s, id).characters, "Jld");
    }

    #[test]
    fn text_commands_apply_to_edited_range() {
        let (mut s, id) = setup("Hello world");
        begin(&mut s, id, Some(0), false).unwrap();
        set_caret(&mut s, 6, true).unwrap(); // "Hello "
        for (cmd, p) in [
            ("text.underline", json!({})),
            ("text.italic", json!({})),
            ("text.sizeUp", json!({})),
            ("text.trackingUp", json!({})),
            ("text.lineHeightUp", json!({})),
            ("text.caseSmallCaps", json!({})),
            ("text.link", json!({"url": "https://example.com"})),
        ] {
            s.execute(cmd, &p).unwrap_or_else(|e| panic!("{cmd}: {e}"));
        }
        let t = data(&s, id);
        let (a, b) = (t.style_at(0), t.style_at(8));
        assert_eq!(a.decoration, TextDecoration::Underline);
        assert!(a.italic() && !b.italic());
        assert_eq!(a.font_size, 25.0);
        assert_eq!(b.font_size, 24.0);
        assert_eq!(a.case, TextCase::SmallCaps);
        assert_eq!(a.link.as_deref(), Some("https://example.com"));
        assert!(b.link.is_none());
        // text.style with an explicit range (what the panel sends).
        s.execute("text.style", &json!({"id": id, "start": 6, "end": 11, "fontSize": 40, "textCase": "upper"})).unwrap();
        let t = data(&s, id);
        assert_eq!(t.style_at(8).font_size, 40.0);
        assert_eq!(t.style_at(8).case, TextCase::Upper);
        // Paragraph commands hit the edited node.
        s.execute("text.listStyle", &json!({"style": "numbered"})).unwrap();
        assert_eq!(data(&s, id).list, ListStyle::Numbered);
        // Every Text-menu command runs while editing.
        for item in crate::catalog::MENU.iter().filter(|i| i.id.starts_with("text.")) {
            let p = if item.id == "text.link" { json!({"url": "u"}) } else { json!({}) };
            if crate::find_command(item.id).is_some() {
                s.execute(item.id, &p).unwrap_or_else(|e| panic!("{}: {e}", item.id));
            }
        }
        end(&mut s).unwrap();
    }

    #[test]
    fn paragraph_and_feature_commands_on_nodes() {
        let (mut s, id) = setup("Hello");
        let ids = json!([id]);
        s.execute("text.setFeatures", &json!({"ids": ids, "features": {"liga": false, "tnum": true}})).unwrap();
        assert_eq!(data(&s, id).features.get("liga"), Some(&false));
        s.execute("text.setFeatures", &json!({"ids": ids, "features": {"liga": null, "ss01": 1}})).unwrap();
        let f = data(&s, id).features;
        assert_eq!(f.get("liga"), None);
        assert_eq!(f.get("ss01"), Some(&true));
        assert_eq!(f.get("tnum"), Some(&true));
        s.execute("text.setFeatures", &json!({"ids": ids, "features": {"zero": true}, "replace": true})).unwrap();
        assert_eq!(data(&s, id).features.len(), 1);
        assert!(s.execute("text.setFeatures", &json!({"ids": ids, "features": {"ligatures": true}})).is_err());
        assert!(s.execute("text.setFeatures", &json!({"ids": ids, "features": {"liga": "yes"}})).is_err());
        assert!(s.execute("text.setFeatures", &json!({"ids": ids})).is_err());
        s.execute("node.set", &json!({"id": id, "props": {"features": {"calt": false}}})).unwrap();
        assert_eq!(data(&s, id).features.len(), 1);
        s.execute("text.paragraphIndent", &json!({"ids": ids, "indent": 16})).unwrap();
        s.execute("text.paragraphSpacing", &json!({"ids": ids, "spacing": 8})).unwrap();
        s.execute("text.maxLines", &json!({"ids": ids, "lines": 2})).unwrap();
        s.execute("text.decoration", &json!({"ids": ids, "decoration": "strikethrough"})).unwrap();
        s.execute("text.case", &json!({"ids": ids, "case": "smallCaps"})).unwrap();
        s.execute("text.listStyle", &json!({"ids": ids, "style": "bulleted"})).unwrap();
        let t = data(&s, id);
        assert_eq!((t.paragraph_indent, t.paragraph_spacing, t.max_lines), (16.0, 8.0, 2));
        assert_eq!(t.auto_resize, TextAutoResize::Truncate);
        assert_eq!(t.style.decoration, TextDecoration::Strikethrough);
        assert_eq!(t.style.case, TextCase::SmallCaps);
        assert_eq!(t.list, ListStyle::Bulleted);
        assert!(s.execute("text.listStyle", &json!({"ids": ids, "style": "zigzag"})).is_err());
        assert!(s.execute("text.paragraphIndent", &json!({"ids": ids, "indent": "NaN"})).is_err());
        s.execute("node.set", &json!({"id": id, "props": {"textCase": "smallCaps", "paragraphIndent": 4, "maxLines": 0}})).unwrap();
        assert_eq!(data(&s, id).paragraph_indent, 4.0);
    }

    #[test]
    fn text_commands_survive_junk_while_editing() {
        let junk = [json!({}), json!(null), json!({"index": -5, "features": {"": 1}}), json!({"index": 1e300, "style": 3, "case": null}), json!([1])];
        for spec in crate::cmd::all_commands().iter().filter(|c| c.id.starts_with("text.")) {
            for j in &junk {
                let (mut s, id) = setup("Hi there\nyou");
                begin(&mut s, id, Some(2), false).unwrap();
                set_caret(&mut s, 5, true).unwrap();
                if let Err(EngineError::Internal { cmd, msg }) = s.execute(spec.id, j) {
                    panic!("{cmd} panicked on {j}: {msg}");
                }
            }
        }
    }
}
