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

/// Type `text` replacing the selection.
pub fn insert(s: &mut Session, text: &str) -> Result<()> {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
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
    let cur = te.caret;
    let next = match key {
        "Left" if !shift && a != b => a,
        "Right" if !shift && a != b => b,
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
    let mut b = a;
    while a > 0 && chars.get(a - 1).is_some_and(|c| is_word(*c)) {
        a -= 1;
    }
    while b < chars.len() && chars.get(b).is_some_and(|c| is_word(*c)) {
        b += 1;
    }
    let ds = s.ds_mut()?;
    if let Some(te) = ds.text_edit.as_mut() {
        te.anchor = a;
        te.caret = b;
    }
    ds.revision += 1;
    Ok(())
}

/// The selected range while editing (for styling commands), or None.
pub fn selected_range(s: &Session) -> Option<(NodeId, usize, usize)> {
    let te = s.ds().ok()?.text_edit.clone()?;
    let (a, b) = range(&te);
    Some((te.id, a, b))
}
