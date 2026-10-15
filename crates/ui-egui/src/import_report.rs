//! The import report: what a successful `.fig`, REST JSON or SVG import left out or changed, shown
//! in a dialog window after the file opens, so the notes don't vanish with a toast.

use egui::RichText;
use serde_json::{Value, json};

use crate::FigApp;
use crate::i18n::{tr, tr_noop, trn};
use crate::theme::{Tokens, regular};

/// A report stays small even when a damaged input produces many or very long notes.
pub const MAX_NOTES: usize = 500;
pub const MAX_NOTE_CHARS: usize = 2_000;

/// The fixed notes of the `.fig` and REST importers, translated in the report. `.fig` imports name
/// "the file" where REST imports name the JSON they read. Other notes (SVG's, say) are shown as they are.
const IMPORT_NOTES: &[&str] = &[
    tr_noop("conditional prototype actions nested too deeply were skipped"),
    tr_noop("conditional prototype actions with conditions that can't be evaluated were skipped"),
    tr_noop("connector labels were skipped"),
    tr_noop("connectors whose ends can't be placed are empty"),
    tr_noop("damaged vector geometry was skipped"),
    tr_noop("damaged vector networks import as outlines"),
    tr_noop("document children other than pages were skipped"),
    tr_noop("elbowed and curved connectors without a saved path import as straight lines"),
    tr_noop("entries without a node type were skipped"),
    tr_noop("filled areas of a vector with their own fill use the layer's fill"),
    tr_noop("gamepad triggers without exactly one known controller button were skipped"),
    tr_noop("grid tracks of fixed or content size import as equal tracks"),
    tr_noop("instance swap properties whose default component isn't in the file were skipped"),
    tr_noop("instances inside their own main component were imported as frames"),
    tr_noop("instances of components that aren't in the file show no layers"),
    tr_noop("instances of components that aren't in the file were imported as frames"),
    tr_noop("instances that would nest a component inside itself were imported as frames"),
    tr_noop("instances without a main component were imported as empty frames"),
    tr_noop("invalid or oversized vector networks import as outlines"),
    tr_noop("invalid or oversized vector path data was skipped"),
    tr_noop("key triggers without a supported key were skipped"),
    tr_noop("key up triggers import as key presses"),
    tr_noop("layers without a position were placed at their parent's origin"),
    tr_noop("layers without a type were skipped"),
    tr_noop("points with their own stroke join use the layer's"),
    tr_noop("prototype actions of unknown types were skipped"),
    tr_noop("prototype actions without a destination in the file were skipped"),
    tr_noop("prototype expressions on variables that aren't in the variables JSON were skipped"),
    tr_noop("prototype expressions with unsupported functions were skipped"),
    tr_noop("prototype interactions without supported actions were skipped"),
    tr_noop("prototype navigations of unknown types were skipped"),
    tr_noop("prototype transitions of unknown types play instantly"),
    tr_noop("prototype triggers of unknown types were skipped"),
    tr_noop("set-variable actions on variables that aren't in the variables JSON were skipped"),
    tr_noop("set-variable actions with values that can't be evaluated were skipped"),
    tr_noop("set-variable actions with values that don't fit the variable were skipped"),
    tr_noop("set-variable actions without a value were skipped"),
    tr_noop("set-variable-mode actions on collections or modes that aren't in the variables JSON were skipped"),
    tr_noop("stars and polygons without point counts or geometry use the default point count"),
    tr_noop("text style overrides refer to missing table entries"),
    tr_noop("the file has no pages"),
    tr_noop("the variables JSON has no `variableCollections` (expected the local variables response) and was ignored"),
    tr_noop("unknown component property types were skipped"),
    tr_noop("variable aliases to variables that aren't in the variables JSON (or of another type) were skipped"),
    tr_noop("variable bindings to properties the layer doesn't have were skipped"),
    tr_noop("variable bindings to variables that aren't in the variables JSON (library variables) were skipped"),
    tr_noop(
        "variable bindings were skipped: the file JSON has no variable definitions (pass the local variables JSON as `variables`, or embed its `meta` next to the file JSON)",
    ),
    tr_noop("variable bindings whose variable type doesn't fit the property were skipped"),
    tr_noop("variable collections without modes were skipped"),
    tr_noop("variable mode lookups in prototype expressions that aren't a variable and one of its modes were skipped"),
    tr_noop("variable modes of collections that aren't in the file were skipped"),
    tr_noop("variable modes of collections that aren't in the variables JSON were skipped"),
    tr_noop("variable values that don't match their type (or composed colours) were skipped"),
    tr_noop("variables of unknown types were skipped"),
    tr_noop("variables whose collection isn't in the variables JSON were skipped"),
    tr_noop("variables without readable values were skipped"),
    tr_noop("vectors without geometry are empty"),
    tr_noop("vectors without geometry are empty (export the file JSON with geometry=paths)"),
    tr_noop("very long text was cut to 1,048,576 characters"),
    tr_noop("very long vector paths were skipped"),
    tr_noop("video actions are not supported and were skipped"),
    tr_noop("video triggers are not supported and were skipped"),
];

#[derive(Clone, Debug)]
pub struct Report {
    pub name: String,
    pub pages: Option<u64>,
    pub layers: Option<u64>,
    pub images: Option<u64>,
    /// The pages of an imported file were added to the open document.
    pub appended: bool,
    /// An SVG was placed on the current page of the open document.
    pub placed: bool,
    /// The document the report belongs to: it goes when that document closes.
    pub uid: u64,
    pub notes: Vec<String>,
    pub note_count: usize,
}

impl Report {
    /// Only successful `file.open` and `file.import` results with import `warnings` create a report;
    /// a native file's `repairs` are shown by the engine's own message, and an SVG opened without
    /// notes has nothing to report. `params` are the command's, for the file's name.
    pub fn from_result(command: &str, params: &Value, result: &Value) -> Option<Self> {
        if !matches!(command, "file.open" | "file.openRecent" | "file.import") {
            return None;
        }
        let warnings = result.get("warnings").and_then(Value::as_array)?;
        let file_name = params.get("path").and_then(Value::as_str).and_then(|p| std::path::Path::new(p).file_name()).and_then(|n| n.to_str());
        if command != "file.import" && warnings.is_empty() && file_name.is_some_and(|n| n.to_ascii_lowercase().ends_with(".svg")) {
            return None;
        }
        // `file.import` of an SVG places a frame on the current page (no `pages`): its notes
        // are reported, but no pages were appended.
        let appended = command == "file.import" && result.get("pages").is_some();
        if command == "file.import" && !appended && warnings.is_empty() {
            return None;
        }
        let summary = result.get("summary");
        let count = |key: &str| summary.and_then(|s| s.get(key)).and_then(Value::as_u64);
        let name = summary
            .and_then(|s| s.get("name"))
            .and_then(Value::as_str)
            .or(file_name)
            .or_else(|| params.get("name").and_then(Value::as_str))
            .unwrap_or(tr("Imported document"));
        let mut report = Self {
            name: name.chars().take(512).collect(),
            pages: count("pages"),
            layers: count("layers"),
            images: count("images"),
            appended,
            placed: command == "file.import" && !appended,
            uid: 0,
            notes: Vec::new(),
            note_count: 0,
        };
        for note in warnings.iter().filter_map(Value::as_str) {
            report.add_note(note);
        }
        Some(report)
    }

    fn add_note(&mut self, note: &str) {
        self.note_count = self.note_count.saturating_add(1);
        if self.notes.len() < MAX_NOTES {
            let mut text: String = note.chars().take(MAX_NOTE_CHARS).collect();
            if note.chars().nth(MAX_NOTE_CHARS).is_some() {
                text.push('…');
            }
            self.notes.push(text);
        }
    }

    pub fn inspect(&self) -> Value {
        json!({"name": self.name, "pages": self.pages, "layers": self.layers, "images": self.images,
            "appended": self.appended, "placed": self.placed, "warnings": self.notes, "warningCount": self.note_count})
    }
}

/// `note` in the UI language when it is a known import note; a repeat count (` (×N)`) is kept. The
/// stored notes are lower-case fragments; a known note is shown as a sentence, so its first letter
/// is capitalised here (unknown notes are left as they are).
pub(crate) fn note_text(note: &str) -> String {
    let translated = |base: &str| {
        if !IMPORT_NOTES.contains(&base) {
            return base.to_owned();
        }
        let text = tr(base);
        let mut chars = text.chars();
        chars.next().map_or_else(String::new, |first| first.to_uppercase().chain(chars).collect())
    };
    match note.rsplit_once(" (×") {
        Some((base, count)) if count.strip_suffix(')').is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())) => {
            format!("{} (×{count}", translated(base))
        }
        _ => translated(note),
    }
}

/// Show the report for a file the user opened or imported (menus, drops, pastes, recents, the
/// system's open event); commands from the control channel, MCP or scripts never call this.
pub fn capture(app: &mut FigApp, command: &str, params: &Value, result: &Value) {
    if let Some(mut report) = Report::from_result(command, params, result) {
        report.uid = app.session.ds().map_or(0, |d| d.uid);
        app.ui.import_report = Some(report);
    }
}

pub fn show(app: &mut FigApp, ctx: &egui::Context) {
    // The report goes with its document.
    if app.ui.import_report.as_ref().is_some_and(|r| !app.session.docs.iter().any(|d| d.uid == r.uid)) {
        app.ui.import_report = None;
    }
    let Some(report) = &app.ui.import_report else { return };
    let t = Tokens::get(ctx);
    let mut open = true;
    let mut close_clicked = false;
    crate::dialogs::window(tr("Import report"), ctx).open(&mut open).show(ctx, |ui| {
        ui.set_width(420.0);
        ui.label(RichText::new(&report.name).strong());
        ui.label(if report.appended {
            tr("Pages added to the document")
        } else if report.placed {
            tr("Placed on the current page")
        } else {
            tr("Opened as a new document")
        });
        let counts: Vec<String> = [(tr("Pages"), report.pages), (tr("Layers"), report.layers), (tr("Images"), report.images)]
            .into_iter()
            .filter_map(|(label, count)| count.map(|n| format!("{label}: {n}")))
            .collect();
        if !counts.is_empty() {
            ui.label(counts.join(" · "));
        }
        ui.add_space(8.0);
        if report.notes.is_empty() {
            ui.label(tr("No compatibility warnings reported"));
        } else {
            ui.label(RichText::new(tr("Compatibility notes")).strong());
            ui.label(
                RichText::new(tr(
                    "FigCraft doesn't support every feature of these files yet. The notes list what was left out or changed in the imported copy; the original file is unchanged.",
                ))
                .color(t.text_secondary)
                .font(regular(11.0)),
            );
            ui.add_space(4.0);
            egui::ScrollArea::vertical().id_salt("import_report_notes").max_height(260.0).show(ui, |ui| {
                for note in &report.notes {
                    ui.label(note_text(note));
                    ui.add_space(4.0);
                }
            });
        }
        let omitted = report.note_count.saturating_sub(report.notes.len());
        if omitted > 0 {
            ui.label(trn("{n} more notes omitted from this report", omitted, &[]));
        }
        ui.add_space(12.0);
        close_clicked = crate::widgets::button(ui, tr("Close"), true, true).clicked();
    });
    if !open || close_clicked {
        app.ui.import_report = None;
    }
}
