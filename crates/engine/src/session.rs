use super::derived::normalize;
use super::doc_state::{DocState, ENDS_PREVIEW, HistoryEntry, Interaction, PREVIEW_READ_ONLY};
use super::error::{EngineError, Result, bad};
use super::prefs::{Clipboard, Prefs, UiRequest};
use super::*;

/// Whether a command, or a batch wrapping one, imports a file passed as Base64 `data`.
fn command_carries_import_bytes(id: &str, params: &Value) -> bool {
    match id {
        "file.import" => params::get(params, "data").is_some(),
        "command.batch" => params.get("commands").and_then(Value::as_array).is_some_and(|commands| {
            commands.iter().any(|c| {
                c.get("command").and_then(Value::as_str).zip(c.get("params")).is_some_and(|(id, params)| command_carries_import_bytes(id, params))
            })
        }),
        _ => false,
    }
}

impl Session {
    /// A session with one new document.
    pub fn new() -> Self {
        let mut s = Self::empty();
        s.docs.push(DocState::new(Document::new("Untitled"), None));
        s
    }

    /// A session with no documents (the home screen).
    pub fn empty() -> Self {
        Self {
            docs: Vec::new(),
            active: 0,
            clipboard: Clipboard::default(),
            prefs: Prefs::default(),
            tool: tools::ToolState::default(),
            ui_requests: Vec::new(),
            ui_dark: None,
            journal: Vec::new(),
            group_depth: 0,
            closed_in_group: Vec::new(),
            recent_colors: Vec::new(),
            last_command: None,
            message: None,
            libraries: library::Cache::default(),
            plugins: plugins::Runtime::default(),
            live: live::Lives::default(),
            background: background::Background::default(),
        }
    }

    /// The background a new file's page gets: the dark page colour while the interface is dark.
    pub fn new_page_background(&self) -> figcraft_color::Color {
        if self.ui_dark == Some(true) { figcraft_color::Color::DARK_PAGE } else { figcraft_color::Color::DEFAULT_PAGE }
    }

    /// Tell the engine the interface's theme (the UI does, every frame). The first time, a new
    /// file nothing was done to yet (the one the app opened with, made before the theme was
    /// known) takes the theme's page colour; later changes of theme leave existing pages alone.
    pub fn set_ui_dark(&mut self, dark: bool) {
        if self.ui_dark.replace(dark).is_some() || !dark {
            return;
        }
        for ds in &mut self.docs {
            let untouched = ds.path.is_none() && ds.history.undo.is_empty() && ds.history.redo.is_empty() && !ds.is_dirty() && ds.versions.is_empty();
            let pages = ds.doc.pages.clone();
            let blank = pages.iter().all(|p| {
                ds.doc.node(*p).is_some_and(|n| {
                    n.children.is_empty()
                        && matches!(n.kind, figcraft_doc::NodeKind::Page { background, .. } if background == figcraft_color::Color::DEFAULT_PAGE)
                })
            });
            if !(untouched && blank) {
                continue;
            }
            let mut doc = (*ds.doc).clone();
            for p in pages {
                if let Some(figcraft_doc::NodeKind::Page { background, .. }) = doc.node_mut(p).map(|n| &mut n.kind) {
                    *background = figcraft_color::Color::DARK_PAGE;
                }
            }
            // Still a file without changes.
            ds.doc = Arc::new(doc);
            ds.saved_doc = ds.doc.clone();
            ds.revision += 1;
        }
    }

    pub fn with_document(doc: Document) -> Self {
        let mut s = Self::empty();
        s.docs.push(DocState::new(doc, None));
        s
    }

    /// Files new to the session start with the View toggles the preferences keep.
    pub fn apply_view_prefs(&mut self) {
        for ds in self.docs.iter_mut().filter(|d| !d.view_from_prefs) {
            cmd::view::apply_kept(&self.prefs, &mut ds.view);
            ds.view_from_prefs = true;
        }
    }

    pub fn ds(&self) -> Result<&DocState> {
        self.docs.get(self.active).ok_or(EngineError::NoDocument)
    }

    pub fn ds_mut(&mut self) -> Result<&mut DocState> {
        self.docs.get_mut(self.active).ok_or(EngineError::NoDocument)
    }

    pub fn doc(&self) -> Result<&Document> {
        Ok(self.ds()?.doc.as_ref())
    }

    pub fn doc_mut(&mut self) -> Result<&mut Document> {
        Ok(self.ds_mut()?.doc_mut())
    }

    pub fn selection(&self) -> Vec<NodeId> {
        self.ds().map(|d| d.selection.clone()).unwrap_or_default()
    }

    pub fn set_selection(&mut self, ids: Vec<NodeId>) {
        if let Ok(ds) = self.ds_mut() {
            ds.selection = ids;
            ds.clean_selection();
            ds.revision += 1;
        }
    }

    pub fn page(&self) -> Result<NodeId> {
        Ok(self.ds()?.page)
    }

    /// Run a command by id. Mutations become one undo step; a failure or panic leaves the document
    /// as it was.
    pub fn execute(&mut self, id: &str, params: &Value) -> Result<Value> {
        let spec = find_command(id).ok_or_else(|| EngineError::UnknownCommand(id.to_string()))?;
        board::refuse_on_board(self, id)?;
        // Revert replaces the document's uid: refuse before ending previews or syncing branches.
        if id == "file.revert" && self.live().doc_uid.is_some() {
            return Err(EngineError::Disabled(id.into(), "leave the live session before reverting to the saved file".into()));
        }
        branches::sync(self);
        self.apply_view_prefs();
        if textpreview::COMMANDS.contains(&id) {
            // A live preview is no edit: no undo step, normalisation or rollback here.
            let r = guard::run(id, || (spec.run)(self, params));
            if r.is_err() {
                textpreview::end(self);
            }
            return r;
        }
        // Every other command acts on the document itself, not on a live text preview.
        textpreview::end(self);
        if ENDS_PREVIEW.contains(&id)
            && (!matches!(id, "file.save" | "file.saveAs") || cmd::file::save_ends_preview(self, id, params))
            && let Ok(ds) = self.ds_mut()
        {
            ds.end_preview();
        }
        let before = self.ds().ok().map(|d| (d.doc.clone(), d.selection.clone(), d.page, d.uid));
        // Cut and copy write the clipboard before a later check can refuse the command.
        let clipboard_before = self.clipboard.clone();
        let result = guard::run(id, || (spec.run)(self, params))
            .and_then(|v| self.check_instance_structure(id, &before).map(|()| v))
            .and_then(|v| self.check_preview(id, &before).map(|()| v))
            .and_then(|v| self.check_live_access(id, &before).map(|()| v))
            .and_then(|v| self.check_finite_geometry(id, &before).map(|()| v));
        match &result {
            Ok(_) => {
                self.after_command(spec, before);
                self.apply_view_prefs();
                if spec.history && id != "edit.repeat" {
                    self.last_command = Some((id.to_string(), params.clone()));
                }
                // Imported bytes already live in the document: a Base64 copy in the log would stay
                // for the whole session.
                if !command_carries_import_bytes(id, params) {
                    self.journal.push((id.to_string(), params.clone()));
                    if self.journal.len() > 5000 {
                        self.journal.drain(..1000);
                    }
                }
            }
            Err(_) => {
                // Roll back any partial edit — also during an interaction, where `before` is the
                // interaction's last valid preview: restore it and leave the gesture itself (its
                // own snapshot) alone.
                if let Some((doc, sel, page, uid)) = before
                    && let Some(ds) = self.docs.iter_mut().find(|d| d.uid == uid)
                {
                    ds.doc = doc;
                    ds.selection = sel;
                    ds.page = page;
                }
                self.clipboard = clipboard_before;
            }
        }
        result
    }

    /// A version shown in place of the document is read-only: refuse an edit (it is rolled back).
    fn check_preview(&self, id: &str, before: &Option<(Arc<Document>, Vec<NodeId>, NodeId, u64)>) -> Result<()> {
        let Some((bdoc, _, _, uid)) = before else { return Ok(()) };
        let Some(ds) = self.docs.iter().find(|d| d.uid == *uid) else { return Ok(()) };
        if ds.preview.is_none() || Arc::ptr_eq(&ds.doc, bdoc) || id == "version.preview" {
            return Ok(());
        }
        Err(EngineError::Disabled(id.to_string(), PREVIEW_READ_ONLY.into()))
    }

    /// A layer's position and size stay finite: infinity and NaN are written as `null`, so a file
    /// holding them saves but can't be reopened. Refuse such an edit (it is rolled back).
    fn check_finite_geometry(&self, id: &str, before: &Option<(Arc<Document>, Vec<NodeId>, NodeId, u64)>) -> Result<()> {
        let Some((bdoc, _, _, uid)) = before else { return Ok(()) };
        let Some(ds) = self.docs.iter().find(|d| d.uid == *uid) else { return Ok(()) };
        if Arc::ptr_eq(&ds.doc, bdoc) {
            return Ok(());
        }
        let broken = |n: &Node| !(n.transform.as_coeffs().iter().all(|c| c.is_finite()) && n.width.is_finite() && n.height.is_finite());
        let untouched = |n: &Node| bdoc.node_arc(n.id).zip(ds.doc.node_arc(n.id)).is_some_and(|(a, b)| Arc::ptr_eq(a, b));
        match ds.doc.nodes().find(|n| broken(n) && !untouched(n)) {
            Some(n) => Err(bad(id, format!("\"{}\" would get a position or size beyond the number range", n.name))),
            None => Ok(()),
        }
    }

    /// A live session's document you may only view (or that is on its way to a server) is
    /// read-only: refuse an edit (it is rolled back).
    fn check_live_access(&self, id: &str, before: &Option<(Arc<Document>, Vec<NodeId>, NodeId, u64)>) -> Result<()> {
        let Some((bdoc, _, _, uid)) = before else { return Ok(()) };
        let Some(ds) = self.docs.iter().find(|d| d.uid == *uid) else { return Ok(()) };
        match self.live_read_only(*uid) {
            Some(why) if !Arc::ptr_eq(&ds.doc, bdoc) => Err(EngineError::Disabled(id.to_string(), why.into())),
            _ => Ok(()),
        }
    }

    /// Instances can't gain, lose or reorder layers, nor move, resize or re-constrain the layers in
    /// them, and library copies are read-only: refuse such an edit (it is rolled back).
    pub(crate) fn check_instance_structure(&self, id: &str, before: &Option<(Arc<Document>, Vec<NodeId>, NodeId, u64)>) -> Result<()> {
        let Some((bdoc, _, _, uid)) = before else { return Ok(()) };
        let Some(ds) = self.docs.iter().find(|d| d.uid == *uid) else { return Ok(()) };
        if Arc::ptr_eq(&ds.doc, bdoc) {
            return Ok(());
        }
        // "Repeat" runs the repeated command.
        let effective = match (&self.last_command, id) {
            (Some((last, _)), "edit.repeat") => last.as_str(),
            _ => id,
        };
        library::check_read_only(effective, bdoc, &ds.doc)
            .and_then(|()| components::check_structure_edit(bdoc, &ds.doc))
            .and_then(|()| components::check_override_edit(bdoc, &ds.doc))
            .map_err(|m| EngineError::Disabled(id.to_string(), m))
    }

    /// Bookkeeping after a successful command: normalise what changed, push undo, bump revision.
    fn after_command(&mut self, spec: &CommandSpec, before: Option<(Arc<Document>, Vec<NodeId>, NodeId, u64)>) {
        let Some((bdoc, bsel, bpage, uid)) = before else {
            if let Ok(ds) = self.ds_mut() {
                ds.revision += 1;
            }
            return;
        };
        let Some(ds) = self.docs.iter_mut().find(|d| d.uid == uid) else { return };
        let changed = !Arc::ptr_eq(&ds.doc, &bdoc);
        // Saving may adopt the chosen filename as the document title. The encoded snapshot is
        // already final; normalising it again would detach it from the saved baseline.
        if changed && !matches!(spec.id, "file.save" | "file.saveAs") {
            normalize(ds.doc_mut(), &bdoc);
        }
        let edit = changed && ds.interaction.is_none();
        ds.clean_selection();
        ds.revision += 1;
        if edit {
            self.record(uid, spec.history.then_some(spec.label), bdoc, bsel, bpage);
        }
    }

    /// Record an edit of document `uid` (it was `before`, shown with `selection` on `page`) as one
    /// undo step called `label` (`None`: not undoable). On a live session's document the edit is
    /// synced and the step is the user's own ([`live`]); otherwise it is a snapshot of `before`.
    pub(crate) fn record(&mut self, uid: u64, label: Option<&str>, before: Arc<Document>, selection: Vec<NodeId>, page: NodeId) {
        if self.live_doc(uid) {
            return self.live_edit(uid, label);
        }
        if let (Some(label), Some(ds)) = (label, self.docs.iter_mut().find(|d| d.uid == uid)) {
            ds.push_undo(label, before, selection, page);
        }
    }

    /// How many undo and redo steps the active document has (the user's own in a live session).
    pub fn undo_depth(&self) -> (usize, usize) {
        match self.ds() {
            Ok(ds) if self.live_doc(ds.uid) => self.live().client().map_or((0, 0), figcraft_collab::Client::history_len),
            Ok(ds) => (ds.history.undo.len(), ds.history.redo.len()),
            Err(_) => (0, 0),
        }
    }

    /// The labels of the active document's last `n` undo steps, the next one first.
    pub fn undo_labels(&self, n: usize) -> Vec<String> {
        match self.ds() {
            Ok(ds) if self.live_doc(ds.uid) => {
                self.live().client().map(|c| c.undo_labels().rev().take(n).map(str::to_string).collect()).unwrap_or_default()
            }
            Ok(ds) => ds.history.undo.iter().rev().take(n).map(|h| h.label.clone()).collect(),
            Err(_) => Vec::new(),
        }
    }

    /// Start a gesture: later [`preview`](Self::preview) calls re-apply edits to this snapshot.
    pub fn begin_interaction(&mut self, label: &str) -> Result<()> {
        textpreview::end(self);
        if let Some(why) = self.active_read_only() {
            return Err(EngineError::Disabled(label.to_string(), why.into()));
        }
        let ds = self.ds_mut()?;
        if ds.preview.is_some() {
            return Err(EngineError::Disabled(label.to_string(), PREVIEW_READ_ONLY.into()));
        }
        if ds.interaction.is_none() {
            ds.interaction = Some(Interaction { label: label.to_string(), doc: ds.doc.clone(), selection: ds.selection.clone() });
        }
        Ok(())
    }

    /// Replace the document with `f(snapshot)` for a live preview during a gesture.
    pub fn preview(&mut self, f: impl FnOnce(&mut Document, &mut Vec<NodeId>) -> Result<()>) -> Result<()> {
        let ds = self.ds_mut()?;
        let Some(it) = ds.interaction.clone() else { return Err(EngineError::Other("no gesture in progress".into())) };
        let mut doc = (*it.doc).clone();
        let mut sel = it.selection.clone();
        // What the gesture changes on the selected layers is an explicit edit.
        let before = cmd::styles::bound_before(&doc, &it.selection);
        let res = f(&mut doc, &mut sel);
        if res.is_ok() {
            cmd::styles::detach_edited(&mut doc, &before);
            normalize(&mut doc, &it.doc);
            // The same preview again (a handle held still) keeps the document: a new one would
            // re-render the canvas for nothing.
            if doc == *ds.doc && sel == ds.selection {
                return res;
            }
            ds.doc = Arc::new(doc);
            ds.selection = sel;
            ds.clean_selection();
            ds.revision += 1;
        }
        res
    }

    /// Finish a gesture as one undo step (no step if nothing changed).
    pub fn commit_interaction(&mut self) -> Result<()> {
        let ds = self.ds_mut()?;
        let Some(it) = ds.interaction.take() else { return Ok(()) };
        let (uid, page) = (ds.uid, ds.page);
        ds.revision += 1;
        if !Arc::ptr_eq(&it.doc, &ds.doc) && *it.doc != *ds.doc {
            self.record(uid, Some(&it.label), it.doc, it.selection, page);
        } else {
            ds.doc = it.doc;
        }
        Ok(())
    }

    /// Abandon a gesture, restoring the document.
    pub fn cancel_interaction(&mut self) -> Result<()> {
        let ds = self.ds_mut()?;
        if let Some(it) = ds.interaction.take() {
            ds.doc = it.doc;
            ds.selection = it.selection;
            ds.revision += 1;
        }
        Ok(())
    }

    pub fn undo(&mut self) -> Result<bool> {
        textpreview::end(self);
        let live = self.ds().is_ok_and(|d| self.live_doc(d.uid));
        let ds = self.ds_mut()?;
        if ds.preview.is_some() {
            return Err(EngineError::Disabled("edit.undo".into(), PREVIEW_READ_ONLY.into()));
        }
        if let Some(it) = ds.interaction.take()
            && live
        {
            // The gesture in progress is dropped (it was never synced).
            ds.doc = it.doc;
        }
        ds.text_edit = None;
        if live {
            return Ok(self.live_travel(true));
        }
        let Some(e) = ds.history.undo.pop() else { return Ok(false) };
        let redo = HistoryEntry { label: e.label.clone(), doc: ds.doc.clone(), selection: ds.selection.clone(), page: ds.page };
        ds.doc = e.doc;
        cmd::facilitate::keep(&redo.doc, &mut ds.doc);
        ds.history.redo.push(redo);
        ds.selection = e.selection;
        if ds.doc.pages.contains(&e.page) {
            ds.page = e.page;
        }
        ds.clean_selection();
        ds.revision += 1;
        let (uid, doc) = (ds.uid, ds.doc.clone());
        if self.clipboard.source_doc == uid {
            self.clipboard.copied_components.retain(|id| {
                doc.node(*id)
                    .is_some_and(|n| matches!(n.kind, figcraft_doc::NodeKind::Component { .. } | figcraft_doc::NodeKind::ComponentSet { .. }))
            });
        }
        Ok(true)
    }

    pub fn redo(&mut self) -> Result<bool> {
        textpreview::end(self);
        let live = self.ds().is_ok_and(|d| self.live_doc(d.uid));
        let ds = self.ds_mut()?;
        if ds.preview.is_some() {
            return Err(EngineError::Disabled("edit.redo".into(), PREVIEW_READ_ONLY.into()));
        }
        if let Some(it) = ds.interaction.take()
            && live
        {
            ds.doc = it.doc;
        }
        ds.text_edit = None;
        if live {
            return Ok(self.live_travel(false));
        }
        let Some(e) = ds.history.redo.pop() else { return Ok(false) };
        let undo = HistoryEntry { label: e.label.clone(), doc: ds.doc.clone(), selection: ds.selection.clone(), page: ds.page };
        ds.doc = e.doc;
        cmd::facilitate::keep(&undo.doc, &mut ds.doc);
        ds.history.undo.push(undo);
        ds.selection = e.selection;
        if ds.doc.pages.contains(&e.page) {
            ds.page = e.page;
        }
        ds.clean_selection();
        ds.revision += 1;
        Ok(true)
    }

    /// Queue a UI request (dialogs etc.).
    pub fn request_ui(&mut self, kind: &str, data: Value) {
        self.ui_requests.push(UiRequest { kind: kind.to_string(), data });
    }

    /// Run several commands as one undo step. A param value `{"$step": n}` is replaced by the `id`
    /// in the result of the earlier step `n`, and `{"$ref": "name"}` by the `id` in the result of
    /// the earlier step whose `as` is `name`; either takes a `field`, a key or slash path (`ids/0`)
    /// into that result. So a step can target a layer that an earlier step of the same batch
    /// created. Any object with a `$step` or `$ref` key in a step's params is such a reference,
    /// except inside a nested `command.batch` step: its references are to its own steps.
    pub fn batch(&mut self, label: &str, steps: &[BatchStep]) -> Result<Vec<Value>> {
        let mut scope = BatchScope::new(steps)?;
        self.grouped(label, |s| {
            for step in steps {
                s.batch_step(&mut scope, &step.command, &step.params)?;
            }
            Ok(scope.into_results())
        })
    }

    /// The next step of a batch: the references in `params` resolve against `scope` (the steps
    /// before it), the command runs and its result joins `scope`. [`Session::batch`] runs a list
    /// of these as one undo step; a caller with its own list — the CLI's script runner — uses this
    /// so its steps resolve references the same way, inside its own [`Session::grouped`].
    pub fn batch_step(&mut self, scope: &mut BatchScope, cmd: &str, params: &Value) -> Result<()> {
        let p = if cmd == "command.batch" { params.clone() } else { scope.resolve(params)? };
        // Say which step failed; a nested batch names its own steps.
        let v = self.execute(cmd, &p).map_err(|e| if cmd == "command.batch" { e } else { e.at_step(scope.next_step()) })?;
        scope.push(v);
        Ok(())
    }

    /// Take the tabs `pred` matches out of `docs`; inside a group they are kept so that a failed
    /// group can reopen them.
    pub(crate) fn remove_tabs(&mut self, mut pred: impl FnMut(&DocState) -> bool) {
        let (gone, keep): (Vec<_>, Vec<_>) = std::mem::take(&mut self.docs).into_iter().partition(|d| pred(d));
        self.docs = keep;
        if self.group_depth > 0 {
            self.closed_in_group.extend(gone);
        }
    }

    /// Run `f` as one undo step called `label`: the commands it executes push no steps of their
    /// own, and when it fails the documents stay at their group-start state.
    pub fn grouped<T>(&mut self, label: &str, f: impl FnOnce(&mut Self) -> Result<T>) -> Result<T> {
        self.ds()?;
        let before: Vec<_> =
            self.docs.iter().map(|ds| (ds.uid, ds.doc.clone(), ds.selection.clone(), ds.page, ds.history.undo.len(), ds.history.limit)).collect();
        // An edit inside clears the redo steps; a failed group has to bring them back.
        let redo_before: Vec<_> = self.docs.iter().map(|ds| ds.history.redo.clone()).collect();
        let active = self.active;
        let journal_len = self.journal.len();

        // The steps inside are dropped again below, so they mustn't push the oldest ones out.
        for ds in &mut self.docs {
            ds.history.limit = usize::MAX;
        }
        for (uid, _, _, _, _, _) in &before {
            self.live_hold(*uid, true);
        }
        self.group_depth += 1;
        let result = f(self);
        self.group_depth -= 1;
        for (uid, _, _, _, _, _) in &before {
            self.live_hold(*uid, false);
        }

        if result.is_err() {
            // Tabs the group closed come back from the stash, and the group-start order returns.
            let mut current = std::mem::take(&mut self.docs);
            for (uid, ..) in &before {
                let tab = match current.iter().position(|d| d.uid == *uid) {
                    Some(i) => Some(current.swap_remove(i)),
                    None => self.closed_in_group.iter().position(|d| d.uid == *uid).map(|i| self.closed_in_group.swap_remove(i)),
                };
                self.docs.extend(tab);
            }
            for ((uid, doc, selection, page, undo_len, limit), redo) in before.iter().zip(redo_before) {
                if let Some(ds) = self.docs.iter_mut().find(|ds| ds.uid == *uid) {
                    ds.history.redo = redo;
                    ds.doc = doc.clone();
                    ds.selection = selection.clone();
                    ds.page = *page;
                    ds.history.undo.truncate(*undo_len);
                    ds.history.limit = *limit;
                }
            }
            self.active = active;
            self.journal.truncate(journal_len);
            if self.group_depth == 0 {
                self.closed_in_group.clear();
            }
            return result;
        }
        if self.group_depth == 0 {
            self.closed_in_group.clear();
        }

        let changed: Vec<_> = before
            .iter()
            .filter_map(|(uid, doc, selection, page, _, _)| {
                let ds = self.docs.iter().find(|d| d.uid == *uid)?;
                (!Arc::ptr_eq(doc, &ds.doc) && ds.interaction.is_none()).then(|| (*uid, doc.clone(), selection.clone(), *page))
            })
            .collect();
        for ds in &mut self.docs {
            if let Some((_, _, _, _, undo_len, limit)) = before.iter().find(|before| before.0 == ds.uid) {
                ds.history.limit = *limit;
                ds.history.undo.truncate(*undo_len);
            }
        }
        for (uid, doc, selection, page) in changed {
            self.record(uid, Some(label), doc, selection, page);
        }
        result
    }

    /// A summary for status displays.
    pub fn status(&self) -> Value {
        match self.ds() {
            Ok(ds) => json!({
                "document": ds.doc.name,
                "path": ds.path,
                "dirty": ds.is_dirty(),
                "page": ds.page,
                "selection": ds.selection,
                "tool": self.tool.tool.id(),
                "zoom": ds.view.zoom,
                "undo": self.undo_depth().0,
                "redo": self.undo_depth().1,
                "documents": self.docs.len(),
            }),
            Err(_) => json!({"documents": 0}),
        }
    }
}
