use super::clock::{AUTO_SAVE_RETRY_SECS, AUTO_SAVE_SECS, AUTOSAVE_SECS, CJK_CHANGES, FrameClock, TOAST_SECS, ToastClock, repaint_in};
use super::open::OPEN_EXTENSIONS;
use super::services::{DialogFuture, PickFuture, Picked, RepaintWaker, Services};
use super::state::{LEFT_PANEL, LeftTab, MOBILE_STRIP, MobilePanel, NARROW_WIDTH, PANEL_DEFAULT, RIGHT_PANEL, RightTab, TitleBar, fit_panels};
use super::*;

impl FigApp {
    /// Construct `FigApp` from a session and services, taking panel widths and pages height
    /// from preferences and initializing UI, canvas, clocks, capture/IME and transient channel
    /// state.
    pub fn new(session: Session, services: Box<dyn Services>) -> Self {
        // The main window's panels open as wide as they were left.
        let (left_width, right_width) = session.prefs.panel_widths;
        let pages_height = session.prefs.pages_height;
        Self {
            ui: UiState { left_width, right_width, pages_height, ..UiState::default() },
            session,
            canvas: canvas::CanvasState::default(),
            services,
            worker: None,
            fonts_installed: false,
            script_fonts: None,
            cjk_checked: None,
            language: None,
            field_focus: false,
            panels_unsaved: false,
            control_queue: Vec::new(),
            control_input: Vec::new(),
            frame_stats: frame_stats::FrameStats::default(),
            windows: windows::Windows::default(),
            window_requests: Vec::new(),
            waiting: Vec::new(),
            pending_dialog: None,
            ui_context: None,
            automation_roots: None,
            control_input_doc: None,
            control_synthetic: false,
            input_acks: Vec::new(),
            applied_acks: Vec::new(),
            applied_input: None,
            control_input_queued: false,
            applied_frame: None,
            clock: FrameClock::default(),
            capture: control::CaptureState::default(),
            logic_ticked: false,
            ui_frame_stamp: None,
            ime: egui::ime_context::ImeContextTracker::new(),
            updates_rt: updates_ui::Runtime::default(),
            #[cfg(test)]
            ime_observations: 0,
        }
    }

    /// Dismiss the composer after a successful switch away from the Comment tool.
    pub(crate) fn clear_comment_draft_after_tool_set(&mut self, id: &str, succeeded: bool) {
        if id == "tool.set" && succeeded && self.session.tool.tool != figcraft_engine::tools::ToolId::Comment {
            self.ui.comments.draft = None;
        }
    }

    /// Run a command, surfacing errors as a toast. Returns the result.
    pub fn run(&mut self, id: &str, params: Value) -> Option<Value> {
        let layers_generation = self.session.clipboard.layers_generation;
        let r = self.session.execute(id, &params);
        self.clear_comment_draft_after_tool_set(id, r.is_ok());
        self.handle_ui_requests();
        self.invalidate_synthetic_copy(layers_generation);
        // A command may change the file the window shows (`doc.switch`, `doc.nextTab`,
        // `file.open`/`file.close`). Record that presentation before returning, so an in-flight
        // screenshot admitted on the old file cannot accept pixels from the new one.
        self.observe_presentation();
        r.map_err(|e| self.failed(id, &e)).ok()
    }

    /// A programmatic command that changed the engine clipboard never wrote the system clipboard:
    /// forget the old system mirror so a later user paste does not trust it as our own copy.
    /// `before` is the layers generation captured before the command ran.
    pub(crate) fn invalidate_synthetic_copy(&mut self, before: u64) {
        if self.control_synthetic && self.session.clipboard.layers_generation != before {
            self.ui.last_copy = None;
        }
    }

    /// Record the file the window drawing now shows, with a monotonic generation that changes on
    /// every switch (an A→B→A switch is two generations, never the admitted one again).
    pub(crate) fn observe_presentation(&mut self) {
        let window = self.windows.current;
        self.windows.presentation(&self.session, window);
    }

    /// [`run`](Self::run) for something the user did (a dialog, a drop, a paste, a recent file):
    /// a file it imports shows its import report. Programmatic calls use `run` and never open one.
    pub fn run_user(&mut self, id: &str, params: Value) -> Option<Value> {
        let result = self.run(id, params.clone());
        if let Some(result) = &result {
            import_report::capture(self, id, &params, result);
        }
        missing_fonts::check_later(self, id, result.as_ref());
        result
    }

    /// Show command `id`'s error.
    pub(crate) fn failed(&mut self, id: &str, e: &figcraft_engine::EngineError) {
        let msg = e.to_string();
        log::warn!("{id}: {msg}");
        self.show_error(msg);
    }

    /// Show an error as a toast, kept as the last error.
    pub(crate) fn show_error(&mut self, msg: String) {
        self.toast(msg.clone());
        self.ui.last_error = Some(msg);
    }

    /// Run a command that may talk to a server without waiting for it (the engine's
    /// [`start_background`](Session::start_background)): `done` gets the result, at once or when
    /// the answer arrives (the network wakes the window then); errors show as with [`run`](Self::run).
    /// [`Session::waiting_for`] tells the panels to show that it's on its way.
    pub fn run_background(&mut self, ctx: &egui::Context, id: &str, params: Value, done: Done) {
        self.waiting.retain(|(c, _)| c != id);
        let r = self.session.start_background(id, params);
        self.handle_ui_requests();
        match r {
            Ok(Some(v)) => done(self, ctx, v),
            Ok(None) => self.waiting.push((id.to_string(), done)),
            Err(e) => self.failed(id, &e),
        }
    }

    /// Stop waiting for command `id` (Cancel, or its window closed): its answer is dropped.
    pub fn cancel_background(&mut self, id: &str) {
        self.session.cancel_background(id);
        self.waiting.retain(|(c, _)| c != id);
    }

    /// Hand the results of background commands that finished to what's waiting for them.
    fn background_done(&mut self, ctx: &egui::Context) {
        while let Some((id, r)) = self.session.take_background_done() {
            self.handle_ui_requests();
            let done = self.waiting.iter().position(|(c, _)| *c == id).map(|i| self.waiting.remove(i).1);
            match (r, done) {
                (Ok(v), Some(done)) => done(self, ctx, v),
                (Ok(_), None) => {}
                (Err(e), _) => self.failed(&id, &e),
            }
        }
    }

    /// Show what `command params` would do on the canvas as a live preview (`text.preview`) while
    /// a font family, font style, text style, named instance or axis slider is hovered or dragged. Call it every frame
    /// the preview should stay: it ends on the first frame nothing asks for it.
    pub fn preview_text(&mut self, ctx: &egui::Context, command: &str, params: Value) {
        self.ui.text_preview_wanted = true;
        let revision = self.session.ds().map_or(0, |d| d.revision);
        if let Some((c, p, rev)) = &self.ui.text_preview
            && c == command
            && *p == params
            && (*rev == revision || figcraft_engine::textpreview::active(&self.session))
        {
            return;
        }
        let shown = self.session.execute("text.preview", &json!({"command": command, "params": params})).is_ok();
        let revision = self.session.ds().map_or(0, |d| d.revision);
        self.ui.text_preview = shown.then(|| (command.to_string(), params, revision));
        // The canvas drew before the panels: show the preview next frame.
        ctx.request_repaint();
    }

    /// End the live text preview the panels show (Escape, or nothing hovered any more).
    pub fn end_text_preview(&mut self, ctx: &egui::Context) {
        if self.ui.text_preview.take().is_some() {
            let _ = self.session.execute("text.endPreview", &json!({}));
            ctx.request_repaint();
        }
    }

    /// Dev Mode (the Inspect tab): the canvas measures on hover without Alt and edits nothing;
    /// entering it ends editing text or vectors.
    fn sync_dev_mode(&mut self) {
        let dev = self.ui.right_tab == RightTab::Inspect;
        if dev && !self.session.tool.dev_mode {
            let _ = self.session.drop_gesture();
            let editing = self.session.ds().map(|d| (d.text_edit.is_some(), d.vector_edit.is_some())).unwrap_or_default();
            for (on, cmd) in [(editing.0, "text.edit.end"), (editing.1, "vector.edit.end")] {
                if on {
                    self.run(cmd, json!({}));
                }
            }
        }
        self.session.tool.dev_mode = dev;
    }

    /// Leaving Design ends a pen path as Escape does and goes back to Move: Prototype and Inspect
    /// don't draw (#307). Checked as the frame starts (the control channel) and after the panels
    /// and shortcuts (the mode switcher, the tabs, Shift+E/Shift+D), before the canvas sees more.
    fn end_pen_outside_design(&mut self) {
        if self.ui.right_tab == self.ui.last_right_tab {
            return;
        }
        self.ui.last_right_tab = self.ui.right_tab;
        if self.ui.right_tab != RightTab::Design && self.session.tool.tool == figcraft_engine::tools::ToolId::Pen {
            self.run("tool.set", json!({"tool": "move"}));
        }
    }

    /// Once a frame, after the panels: a preview nothing asked for this frame ends.
    fn end_unwanted_text_preview(&mut self, ctx: &egui::Context) {
        if !std::mem::take(&mut self.ui.text_preview_wanted) {
            self.end_text_preview(ctx);
        }
    }

    /// Finish the frame that applies an injected batch: acknowledge it once the pass is presented,
    /// and stop fencing the system clipboard. Only the viewport whose batch this frame took acts.
    pub(crate) fn end_input_frame(&mut self, ctx: &egui::Context) {
        if self.applied_frame != Some(ctx.viewport_id()) {
            return;
        }
        self.applied_input = None;
        if ctx.will_discard() {
            // Keep `applied_frame`/`applied_acks`: the rerun pass must present (and acknowledge)
            // the same frame without replaying the input.
            return;
        }
        self.applied_frame = None;
        for ack in self.applied_acks.drain(..) {
            let _ = ack.tx.send(ack.reply);
        }
        self.control_synthetic = false;
    }

    /// The app's monotonic clock in seconds, also usable from logic-only callbacks (recovery,
    /// control deadlines). Never `ctx.input(i.time)`, which is stale there.
    pub fn now_secs(&self) -> f64 {
        self.clock.now()
    }

    /// Queue raw input injected through the control channel for the window drawing next, tagged
    /// with the document it belongs to (a batch is dropped if another file is active by then).
    pub(crate) fn push_control_input(&mut self, events: Vec<egui::Event>) -> bool {
        if events.is_empty() {
            return false;
        }
        let doc = self.session.docs.get(self.session.active).map(|d| d.uid);
        if !self.control_input.is_empty() && self.control_input_doc != doc {
            // A queued batch belongs to the file that was open when it was injected; it cannot be
            // merged with one for the file showing now. Its acknowledgement says so.
            self.control_input.clear();
            let viewport = crate::windows::viewport(self.windows.current);
            crate::windows::fail_input_acks(self, viewport);
        }
        // Always the live pane vector: while a child window draws, `windows::enter` has swapped
        // its pane into the app, and `leave` swaps these events back into that pane.
        self.control_input.extend(events);
        self.control_input_doc = doc;
        self.control_synthetic = true;
        true
    }

    /// The acknowledgements waiting for `viewport`'s input batch (other windows keep theirs).
    pub(crate) fn take_input_acks(&mut self, viewport: egui::ViewportId) -> Vec<PendingAck> {
        let mut mine = Vec::new();
        for ack in std::mem::take(&mut self.input_acks) {
            if ack.viewport == viewport {
                mine.push(ack);
            } else {
                self.input_acks.push(ack);
            }
        }
        mine
    }

    /// Put `text` on the system clipboard. Input injected through the control channel never
    /// replaces what the user copied: this is the one write path every copy command goes through.
    pub(crate) fn clipboard_set_text(&mut self, text: &str) {
        if self.control_synthetic {
            return;
        }
        self.services.clipboard_set_text(text);
    }

    /// [`Self::clipboard_set_text`] for an image (straight RGBA).
    pub(crate) fn clipboard_set_png(&mut self, width: u32, height: u32, rgba: &[u8]) {
        if self.control_synthetic {
            return;
        }
        self.services.clipboard_set_png(width, height, rgba);
    }

    pub fn toast(&mut self, msg: impl Into<String>) {
        self.ui.toast = Some((msg.into(), ToastClock::Raised));
    }

    /// Say that `n` documents left unsaved by a crash were opened again from recovery copies.
    pub fn note_recovered(&mut self, n: usize) {
        if n > 0 {
            self.toast(i18n::trn("Recovered {n} unsaved files after the app quit unexpectedly: save them to keep them", n, &[]));
        }
    }

    /// Act on UI requests queued by commands (dialogs, panels).
    pub fn handle_ui_requests(&mut self) {
        let reqs = std::mem::take(&mut self.session.ui_requests);
        for r in reqs {
            match r.kind.as_str() {
                "dialog.about" => {
                    self.ui.about = true;
                    self.ui.about_tab = dialogs::AboutTab::About;
                }
                "dialog.preferences" => self.ui.preferences = true,
                "dialog.missingFonts" => missing_fonts::request(self),
                "panel.shortcuts" => self.ui.shortcuts_panel = !self.ui.shortcuts_panel,
                "palette.actions" => menus::toggle_palette(&mut self.ui),
                "panel.design" if self.session.doc().is_ok_and(figcraft_engine::board::is_board) => self.ui.board_panel = !self.ui.board_panel,
                "panel.design" => self.ui.right_tab = RightTab::Design,
                "board.editText" => {
                    if let Some(id) = r.data.get("id").and_then(figcraft_engine::params::id_of)
                        && let Err(e) = figcraft_engine::board::edit_text(&mut self.session, id)
                    {
                        self.toast(e.to_string());
                    }
                }
                "panel.prototype" => self.ui.right_tab = RightTab::Prototype,
                "panel.designPrototype" => {
                    self.ui.right_tab = if self.ui.right_tab == RightTab::Prototype { RightTab::Design } else { RightTab::Prototype };
                }
                "panel.devMode" => {
                    self.ui.right_tab = if self.ui.right_tab == RightTab::Inspect { RightTab::Design } else { RightTab::Inspect };
                }
                "panel.leftSidebar" => {
                    let shown = if self.ui.narrow { self.ui.mobile_panel == Some(MobilePanel::Left) } else { !self.ui.left_collapsed };
                    if shown {
                        self.ui.close_left_panel();
                    } else {
                        self.ui.open_left_panel();
                    }
                }
                "panel.widths" => {
                    let width = |k: &str| r.data.get(k).and_then(Value::as_f64).map(|w| w as f32);
                    let (left, right) = (width("left"), width("right"));
                    let reset = left.is_none() && right.is_none();
                    self.set_panel_widths(
                        left.unwrap_or(if reset { PANEL_DEFAULT } else { self.ui.left_width }),
                        right.unwrap_or(if reset { PANEL_DEFAULT } else { self.ui.right_width }),
                    );
                    self.services.save_prefs(&self.session.prefs);
                }
                "focus.region" => self.ui.nav.step(r.data.get("step").and_then(Value::as_i64).unwrap_or(1).clamp(-4, 4) as i32),
                "panel.layers" => self.ui.left_tab = LeftTab::File,
                "panel.assets" => {
                    self.ui.left_tab = LeftTab::Assets;
                    self.ui.open_left_panel();
                    self.ui.versions = Default::default();
                }
                "panel.versionHistory" => versions::open(self),
                "view.devCompare" => {
                    self.ui.dev.compare = dev_compare::CompareView::requested(&r.data);
                }
                "panel.libraries" => {
                    self.ui.left_tab = LeftTab::Assets;
                    self.ui.open_left_panel();
                    self.ui.libraries = true;
                }
                "home" => self.ui.home = true,
                "home.templates" => {
                    self.ui.home = true;
                    self.ui.home_view.section = home::Section::Templates;
                }
                "home.gettingStarted" => {
                    self.ui.home = true;
                    self.ui.home_view.section = home::Section::Recents;
                    self.services.save_prefs(&self.session.prefs);
                }
                "dialog.saveAs" => self.save_as_dialog(),
                "dialog.colorProfile" => self.ui.color_profile = self.session.doc().ok().map(|d| d.color_profile),
                "present" => {
                    if let Some(start) = r.data.get("start").and_then(figcraft_engine::params::id_of) {
                        tabs::open_present(self, start);
                    }
                }
                "present.options" => {
                    let v = &mut self.ui.present_view;
                    if let Some(s) = r.data.get("scale").and_then(|s| s.as_str()).and_then(present::PresentScale::parse) {
                        v.scale = s;
                    }
                    if let Some(f) = r.data.get("deviceFrame").and_then(|f| f.as_bool()) {
                        v.device_frame = f;
                    }
                    if let Some(h) = r.data.get("hotspotHints").and_then(|h| h.as_bool()) {
                        v.hotspot_hints = h;
                    }
                    if let Some(i) = r.data.get("inspect").and_then(|i| i.as_bool()) {
                        v.inspect = i;
                    }
                    if let Some(p) = self.ui.present.as_mut() {
                        p.view.device_frame = v.device_frame;
                        p.view.hotspot_hints = v.hotspot_hints;
                        p.view.inspect = v.inspect;
                        p.set_scale(v.scale);
                    }
                }
                "preview" => {
                    if let Some(start) = r.data.get("start").and_then(figcraft_engine::params::id_of) {
                        self.ui.preview = Some(present::Pane::new(start, &self.session));
                    }
                }
                "preview.close" => self.ui.preview = None,
                "open.help" => self.services.open_url("https://getartcraft.com/apps/figcraft"),
                "layers.collapseAll" => layers::collapse_all(self),
                "contextMenu" => self.ui.key_menu = true,
                "selectArea" => self.start_key_box(),
                "canvas.renameLabel" => {
                    self.ui.rename_label = r.data.get("id").and_then(figcraft_engine::params::id_of).map(|id| layers::Rename { id, start: true });
                }
                "layers.rename" => {
                    let ids: Vec<NodeId> =
                        r.data.get("ids").and_then(Value::as_array).into_iter().flatten().filter_map(figcraft_engine::params::id_of).collect();
                    if ids.len() > 1 {
                        dialogs::open_batch_rename(self, &ids);
                    } else {
                        layers::start_rename(self, &ids);
                    }
                }
                "comment.open" => {
                    if let Some(id) = r.data.get("id").and_then(Value::as_u64) {
                        self.ui.comments.show(id);
                    }
                }
                "comment.new" => {
                    let x = r.data.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0);
                    let y = r.data.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0);
                    comments::start_draft(self, x, y);
                }
                "dialog.unsaved" => self.toast(i18n::tr("This file has unsaved changes")),
                "dialog.plugins" => self.ui.plugins_window = true,
                "dialog.pluginSave" => plugins_ui::save_file(self, &r.data),
                "dialog.cursorChat" => live::open_chat(self),
                "window.new" | "window.focus" | "window.fullscreen" => self.window_requests.push(r),
                _ => {}
            }
        }
        if let Some(m) = self.session.message.take() {
            self.toast(m);
        }
    }

    /// Set the widths asked for this window's side panels (within their limits); the main
    /// window's are kept in the preferences.
    pub fn set_panel_widths(&mut self, left: f32, right: f32) {
        let within = |w: f32, range: std::ops::RangeInclusive<f32>| if w.is_finite() { w.clamp(*range.start(), *range.end()) } else { PANEL_DEFAULT };
        (self.ui.left_width, self.ui.right_width) = (within(left, LEFT_PANEL), within(right, RIGHT_PANEL));
        if self.windows.current == 0 {
            self.session.prefs.panel_widths = (self.ui.left_width, self.ui.right_width);
        }
    }

    /// The edge between a side panel and the canvas, at `x` down `rows`: dragging it sizes the
    /// panel (`grows`: +1 for the left panel, whose edge is on its right, −1 for the right one), a
    /// double-click puts back the default width. Returns the width asked for, when it changed.
    fn panel_edge(ui: &mut egui::Ui, name: &str, x: f32, rows: egui::Rangef, width: f32, grows: f32) -> Option<f32> {
        let grab = ui.style().interaction.resize_grab_radius_side;
        let rect = egui::Rect::from_x_y_ranges(x - grab..=x + grab, rows);
        let id = egui::Id::unique(("panel_edge", name));
        // Not focusable: an edge is a pointer target, so it must not take Tab/F6 focus from the
        // panels' controls (see `a11y::focusables`).
        let resp = ui.interact(rect, id, egui::Sense::CLICK | egui::Sense::DRAG);
        if resp.hovered() || resp.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            ui.painter().vline(x, rows, egui::Stroke::new(2.0, theme::ACCENT));
        }
        if resp.double_clicked() {
            return Some(PANEL_DEFAULT);
        }
        // The edge stays under the pointer where it was grabbed (also back from past a limit).
        let at = resp.interact_pointer_pos().filter(|_| resp.dragged())?;
        if resp.drag_started() {
            let pressed = ui.input(|i| i.pointer.press_origin()).unwrap_or(at);
            ui.data_mut(|d| d.insert_temp(id, pressed.x - x));
        }
        let grabbed: f32 = ui.data(|d| d.get_temp(id)).unwrap_or(0.0);
        let w = width + (at.x - grabbed - x) * grows;
        (w != width).then_some(w)
    }

    pub fn save_as_dialog(&mut self) {
        if self.services.downloads() {
            if let Some(r) = self.run("file.saveAs", json!({"data": true}))
                && self.download(r["name"].as_str().unwrap_or_default(), &r)
            {
                self.run("file.markSaved", json!({}));
                dialogs::file_saved(self, &r);
            }
            return;
        }
        let Ok(name) = self.session.doc().map(|d| d.name.clone()) else { return };
        self.pick_save(i18n::tr("Save as"), &format!("{name}.figcraft"), &["figcraft", "fig"], |app, mut p| {
            if p.extension().is_none_or(OsStr::is_empty) {
                if !p.set_extension("figcraft") {
                    app.toast(i18n::tr("Use the .figcraft or .fig extension."));
                    return;
                }
                // This name wasn't confirmed by the system dialog: don't replace an existing file.
                match p.try_exists() {
                    Ok(false) => {}
                    Ok(true) => {
                        app.toast(i18n::tr("A .figcraft file with this name already exists. Choose its full name to confirm replacement."));
                        return;
                    }
                    Err(e) => {
                        app.toast(e.to_string());
                        return;
                    }
                }
            } else if !p.extension().and_then(OsStr::to_str).is_some_and(|e| e.eq_ignore_ascii_case("figcraft") || e.eq_ignore_ascii_case("fig")) {
                app.toast(i18n::tr("Use the .figcraft or .fig extension."));
                return;
            }
            if let Some(r) = app.run("file.saveAs", json!({"path": p.to_string_lossy()})) {
                dialogs::file_saved(app, &r);
            }
        });
    }

    pub fn save(&mut self) {
        // A branch tab saves its main file.
        let tab = figcraft_engine::branches::file_tab(&self.session);
        // A path ending in .fig is never saved over silently: a .fig destination is an export copy.
        let native_path = self
            .session
            .docs
            .get(tab)
            .and_then(|d| d.path.as_deref())
            .is_some_and(|path| !Path::new(path).extension().and_then(OsStr::to_str).is_some_and(|e| e.eq_ignore_ascii_case("fig")));
        if native_path {
            if self.run("file.save", json!({})).is_some() {
                self.toast(i18n::tr("Saved"));
            }
        } else {
            self.save_as_dialog();
        }
    }

    pub fn open_dialog(&mut self) {
        // Opening needs no file in front: it goes ahead even if that file closed meanwhile.
        self.pick_file(i18n::tr("Open"), OPEN_EXTENSIONS, false, |app, file| {
            if app.run_user("file.open", file).is_some() {
                app.ui.home = false;
            }
        });
    }

    pub fn import_dialog(&mut self, place: bool) {
        let exts: &[&str] = if place {
            &["png", "jpg", "jpeg", "webp", "gif", "tif", "tiff"]
        } else {
            &["svg", "png", "jpg", "jpeg", "webp", "gif", "tif", "tiff", "figcraft", "fig", "sketch"]
        };
        let title = if place { i18n::tr("Place image") } else { i18n::tr("Import") };
        self.pick_file(title, exts, true, |app, file| {
            app.run_user("file.import", file);
        });
    }

    /// Pick a file to open or import; `done` gets it as `file.open` and `file.import` take it (a
    /// path, or the name and contents where the platform has no paths). `pin`: as
    /// [`FigApp::pick_open`]'s file in front.
    fn pick_file(&mut self, title: &str, exts: &[&str], pin: bool, done: impl FnOnce(&mut FigApp, Value) + 'static) {
        self.start_pick(
            |services| -> PickFuture {
                match services.pick_open_contents(title, exts) {
                    Some(read) => {
                        Box::pin(async move { read.await.map(|r| r.map_or_else(Picked::Unreadable, |(name, bytes)| Picked::Contents(name, bytes))) })
                    }
                    None => {
                        let pick = services.pick_open(title, exts);
                        Box::pin(async move { pick.await.map(Picked::Path) })
                    }
                }
            },
            pin,
            move |app, picked| match picked.file_params() {
                Ok(file) => done(app, file),
                Err(why) => app.show_error(why),
            },
        );
    }

    /// Pick a file to open. `done` runs with the file in front now back in front (and not at all
    /// if that file closed meanwhile).
    pub fn pick_open(&mut self, title: &str, exts: &[&str], done: impl FnOnce(&mut FigApp, PathBuf) + 'static) {
        self.start_dialog(|services| services.pick_open(title, exts), true, done);
    }

    /// Pick where to save; `done` runs as [`FigApp::pick_open`]'s does.
    pub fn pick_save(&mut self, title: &str, name: &str, exts: &[&str], done: impl FnOnce(&mut FigApp, PathBuf) + 'static) {
        self.start_dialog(|services| services.pick_save(title, name, exts), true, done);
    }

    /// Hand a command result's base64 `data` over as a download named `name`, where the platform
    /// saves by downloads rather than save dialogs (the browser, #969). Says so when nothing arrived.
    pub(crate) fn download(&mut self, name: &str, result: &Value) -> bool {
        use base64::Engine as _;
        let bytes = result["data"].as_str().and_then(|d| base64::engine::general_purpose::STANDARD.decode(d).ok());
        if bytes.is_some_and(|b| self.services.download(name, &b)) {
            return true;
        }
        self.toast(i18n::trf("Couldn't download {name}", &[("name", &name)]));
        false
    }

    /// Pick a folder; `done` runs as [`FigApp::pick_open`]'s does.
    pub fn pick_folder(&mut self, title: &str, done: impl FnOnce(&mut FigApp, PathBuf) + 'static) {
        self.start_dialog(|services| services.pick_folder(title), true, done);
    }

    /// Open a native dialog from the window drawing now, for the file in front (`pin`).
    fn start_dialog(&mut self, start: impl FnOnce(&mut dyn Services) -> DialogFuture, pin: bool, done: impl FnOnce(&mut FigApp, PathBuf) + 'static) {
        self.start_pick(
            |services| -> PickFuture {
                let pick = start(services);
                Box::pin(async move { pick.await.map(Picked::Path) })
            },
            pin,
            move |app, picked| {
                if let Picked::Path(p) = picked {
                    done(app, p);
                }
            },
        );
    }

    /// [`FigApp::start_dialog`] for any kind of pick.
    fn start_pick(&mut self, start: impl FnOnce(&mut dyn Services) -> PickFuture, pin: bool, done: impl FnOnce(&mut FigApp, Picked) + 'static) {
        if self.pending_dialog.is_some() {
            return;
        }
        let doc = if pin { self.session.ds().ok().filter(|d| self.shows(d.uid)).map(|d| d.uid) } else { None };
        let future = start(self.services.as_mut());
        self.pending_dialog = Some(PendingDialog { future, picked: None, done: Box::new(done), doc, window: self.windows.current });
        if let Some(ctx) = &self.ui_context {
            ctx.request_repaint();
        }
    }

    /// Once the dialog has a path, its continuation runs in the window that holds its file, with
    /// that file in front; then the tab in front before comes back (unless the dialog opened a file).
    pub(crate) fn poll_dialog(&mut self, ctx: &egui::Context) {
        let Some(d) = &mut self.pending_dialog else { return };
        if d.picked.is_none() {
            let waker = Waker::from(Arc::new(RepaintWaker(ctx.clone())));
            let Poll::Ready(picked) = d.future.as_mut().poll(&mut TaskContext::from_waker(&waker)) else { return };
            d.picked = picked;
        }
        let (doc, origin, picked) = (d.doc, d.window, d.picked.is_some());
        let index = doc.map(|u| self.session.docs.iter().position(|ds| ds.uid == u));
        // Cancelled, or its file closed meanwhile.
        if !picked || index == Some(None) {
            self.pending_dialog = None;
            return;
        }
        let window = match doc {
            Some(u) => self.windows.of(u),
            None if self.windows.exists(origin) => origin,
            None => 0,
        };
        if window != self.windows.current {
            ctx.request_repaint_of(windows::viewport(window));
            return;
        }
        let Some(PendingDialog { picked: Some(picked), done, .. }) = self.pending_dialog.take() else { return };
        let before = self.session.ds().ok().map(|d| d.uid);
        if let Some(Some(i)) = index {
            self.session.active = i;
        }
        done(self, picked);
        if doc.is_some()
            && self.session.ds().ok().map(|d| d.uid) == doc
            && let Some(i) = before.and_then(|u| self.session.docs.iter().position(|d| d.uid == u))
        {
            self.session.active = i;
        }
    }

    /// Reduced motion: the preference or the system's setting.
    pub fn reduce_motion(&self) -> bool {
        self.session.prefs.reduce_motion || self.services.reduce_motion()
    }

    /// The CJK, Bengali and Thai scripts the UI shows: the language's, the language picker's (Preferences open),
    /// typed text's, and those in the open file's names and comments (read when it changes, not
    /// mid-drag). Nothing is checked once every script has a face.
    pub(crate) fn cjk_needed(&mut self, ctx: &egui::Context, installed: u8) -> u8 {
        if installed == cjk::ALL {
            return installed;
        }
        let mut need = i18n::current().map_or(0, |l| cjk::language(l.code));
        if self.ui.preferences {
            need |= i18n::languages().iter().filter(|l| cjk::can_show(l.code)).fold(0, |m, l| m | cjk::language(l.code));
        }
        need |= ctx.input(|i| {
            i.events.iter().fold(0, |m, e| match e {
                egui::Event::Text(t) | egui::Event::Ime(egui::ImeEvent::Preedit { text: t, .. } | egui::ImeEvent::Commit(t)) => m | cjk::scripts(t),
                _ => m,
            })
        });
        let dragging = ctx.input(|i| i.pointer.any_down());
        // Pans and zooms change the revision too: only a new version of the document is looked
        // at, and only the layers it changed (a large file has hundreds of thousands of names).
        let current = self.session.docs.get(self.session.active).filter(|_| !dragging).map(|d| (d.uid, d.doc.clone()));
        if let Some((uid, doc)) = current {
            let before = self.cjk_checked.take().filter(|(u, _)| *u == uid).map(|(_, b)| b);
            if !before.as_ref().is_some_and(|b| Arc::ptr_eq(b, &doc)) {
                let mut scan = |s: &str| need |= cjk::scripts(s);
                scan(&doc.name);
                match before.as_ref().and_then(|b| doc.changed_ids(b, CJK_CHANGES)) {
                    Some(ids) => ids.into_iter().filter_map(|id| doc.node(id)).for_each(|n| scan(&n.name)),
                    None => doc.nodes().for_each(|n| scan(&n.name)),
                }
                doc.comments.iter().flat_map(|c| std::iter::once(&c.first).chain(&c.replies)).for_each(|m| {
                    scan(&m.author);
                    scan(&m.text);
                });
            }
            self.cjk_checked = Some((uid, doc));
        }
        need
    }

    /// Work safe from a logic-only callback (`App::logic`, which also runs while hidden) and from
    /// a normal UI pass: consumed control work, live polling, background completions, autosave and
    /// recovery deadlines. It never reads UI input, so it must not scan events, run shortcuts or
    /// lay out anything.
    pub fn background_tick(&mut self, ctx: &egui::Context) {
        control::background(self, ctx);
        // What happened in a live session (the network woke us for it).
        if self.session.live_poll() {
            if let Some(m) = self.session.message.take() {
                self.toast(live::failure_text(&m));
            }
            self.background_done(ctx);
        }
        // Files that arrived without a command (joined, recovered) take the kept View toggles too.
        self.session.apply_view_prefs();
        let now = self.clock.now();
        self.autosave_tick(ctx, now);
        self.auto_save_tick(ctx, now);
        updates_ui::tick(self, ctx);
        self.logic_ticked = true;
    }

    /// UI-phase setup that needs a real current `Ui`: fonts/language/theme, interface zoom and the
    /// render worker. Runs once per frame (pass 0) for the main window, and once per frame for
    /// every child window (`windows::draw`), never on a logic-only tick.
    fn ui_setup(&mut self, ctx: &egui::Context) {
        self.viewport_setup(ctx);
    }

    /// The app-global part of [`Self::ui_setup`] that every window's frame needs: language, fonts,
    /// theme and `ui.dark`, the render worker, and the interface zoom plan. eframe only calls
    /// `App::ui` for the root window, so a child must run this itself before it draws; a child that
    /// only runs while the root is minimised still gets theme switches, CJK faces for its own
    /// document, and the right `ui.dark` for its tabs and dialogs.
    pub(crate) fn viewport_setup(&mut self, ctx: &egui::Context) {
        if self.language.as_ref() != Some(&self.session.prefs.language) {
            i18n::set(&self.session.prefs.language, self.services.system_language().as_deref());
            self.language = Some(self.session.prefs.language.clone());
        }
        // Screen-reader and bidi language of the UI (fork E17).
        ctx.set_language(i18n::current().map(|l| l.code.to_string()));
        if !self.fonts_installed {
            // Cmd/Ctrl+= / - / 0 zoom the canvas (view.zoomIn, view.zoomOut, view.zoom100): egui's
            // own keyboard zoom would scale the whole interface on the same keys.
            ctx.options_mut(|o| o.zoom_with_keyboard = false);
            // A double-click's window is the platform's (0.5 s, Windows' default), not egui's
            // stricter 0.3 s: a double-click the OS takes must enter a shape's points too.
            ctx.memory_mut(|m| m.options.input_options.max_double_click_delay = 0.5);
            theme::install_fonts(ctx, false, 0);
            self.fonts_installed = true;
            // Installed fonts apply from the next frame; a child window draws on here, so ask for
            // that frame (the root draws nothing on the frame it installs them).
            ctx.request_repaint();
        } else if figcraft_text::system_fonts_scanned() {
            // Once the system fonts are indexed: faces for names in scripts the UI fonts lack, and
            // for each CJK script once something shows it.
            let installed = self.script_fonts.map_or(0, |(s, _)| s);
            let cjk = installed | self.cjk_needed(ctx, installed);
            // The UI language's script leads the CJK faces, so another language lays them out again.
            let lang = i18n::current().map_or("en", |l| l.code);
            if self.script_fonts != Some((cjk, lang)) {
                self.script_fonts = Some((cjk, lang));
                theme::install_fonts(ctx, true, cjk);
            }
        }
        // egui's zoom is for the whole app and multiplies each window's own system scale, so the
        // size is the same relative to every screen; the main window, whose size can hold a size
        // back, sets it. The browser keeps its own zoom. Every window records its own plan (the
        // Preferences warning and `ui.inspect` read it), from its own size.
        {
            let window_px = ctx.content_rect().size() * ctx.pixels_per_point();
            let size = if cfg!(target_arch = "wasm32") { 1.0 } else { self.session.prefs.ui_scale };
            let plan = ui_scale::plan(size, ctx.native_pixels_per_point(), Some((window_px.x, window_px.y)));
            if ctx.viewport_id() == egui::ViewportId::ROOT {
                ctx.set_zoom_factor(plan.zoom);
            }
            self.ui.ui_scale = plan;
        }
        let dark = match self.session.prefs.theme.as_str() {
            "dark" => true,
            // The active visuals were set by us last frame (possibly for a manual theme). Read the
            // integration's OS theme; winit reports none on Linux and the BSDs, where the platform
            // services' colour scheme answers instead. Without either, keep the light fallback.
            "system" => ctx.system_theme().map(|t| t == egui::Theme::Dark).or_else(|| self.services.system_dark()).unwrap_or(false),
            _ => false,
        };
        self.ui.dark = dark;
        // New files take the theme's page colour (existing pages keep theirs).
        self.session.set_ui_dark(dark);
        theme::apply_with(ctx, if dark { theme::Tokens::dark() } else { theme::Tokens::light() }, self.reduce_motion());
        if self.worker.is_none() {
            self.worker = Some(render_worker::Worker::new(ctx.clone()));
        }
    }

    /// Every [`AUTOSAVE_SECS`], each open document with unsaved changes since its last version
    /// gets an autosave checkpoint in its version history.
    fn autosave_tick(&mut self, ctx: &egui::Context, now: f64) {
        if now < self.ui.next_autosave {
            // Wake up for it even if nothing else draws a frame by then.
            repaint_in(ctx, self.ui.next_autosave - now);
            return;
        }
        if self.ui.next_autosave > 0.0 {
            let active = self.session.active;
            for i in 0..self.session.docs.len() {
                self.session.active = i;
                let _ = self.session.execute("version.autosave", &json!({}));
            }
            self.session.active = active;
        }
        self.ui.next_autosave = now + AUTOSAVE_SECS;
        repaint_in(ctx, AUTOSAVE_SECS);
    }

    /// With auto-save on, files that have a path are saved [`AUTO_SAVE_SECS`] after they last
    /// changed (not during a gesture, a live preview or while a version is shown). The file is
    /// encoded on a thread of its own, so a big one doesn't hold up the window, and written here.
    fn auto_save_tick(&mut self, ctx: &egui::Context, now: f64) {
        let mut failed = self.auto_save_done();
        // The browser has no files to save to.
        let on = !cfg!(target_arch = "wasm32") && self.session.prefs.auto_save;
        let saving = &self.ui.auto_saving;
        let due = |d: &&figcraft_engine::DocState| {
            d.path.as_deref().is_some_and(|p| d.images_lost_saving_to(p).is_none())
                && d.branch_of.is_none()
                && d.preview.is_none()
                && d.is_dirty()
                && !saving.iter().any(|(uid, _)| *uid == d.uid)
        };
        let changes = if on { self.session.docs.iter().filter(due).map(|d| d.revision).reduce(u64::wrapping_add) } else { None };
        match changes {
            None => self.ui.auto_save_at = None,
            Some(changes) => {
                let at = match self.ui.auto_save_at {
                    Some((at, c)) if c == changes && failed.is_none() => at,
                    _ => now + if failed.is_some() { AUTO_SAVE_RETRY_SECS } else { AUTO_SAVE_SECS },
                };
                self.ui.auto_save_at = Some((at, changes));
                if now < at || self.session.docs.iter().any(|d| d.interaction.is_some() || d.text_preview.is_some()) {
                    // Due but busy: look again in a second.
                    repaint_in(ctx, if now < at { at - now } else { 1.0 });
                } else {
                    self.ui.auto_save_at = None;
                    let files: Vec<_> =
                        self.session.docs.iter().filter(due).map(|d| (d.uid, figcraft_engine::cmd::file::FileSnapshot::of(d))).collect();
                    for (uid, file) in files {
                        let encode = move || {
                            let bytes = file.encode();
                            (file, bytes)
                        };
                        match std::thread::Builder::new().name("figcraft-save".into()).spawn(encode) {
                            Ok(h) => self.ui.auto_saving.push((uid, h)),
                            Err(e) => {
                                failed.get_or_insert(e.to_string());
                                self.ui.auto_save_at = Some((now + AUTO_SAVE_RETRY_SECS, changes));
                            }
                        }
                    }
                }
            }
        }
        if !self.ui.auto_saving.is_empty() {
            // Look again shortly for the files being encoded.
            repaint_in(ctx, 0.1);
        }
        if let Some(e) = failed {
            self.toast(i18n::trf("Couldn't save automatically: {error}", &[("error", &e)]));
        }
    }

    /// Write the files whose auto-save encoding finished; the first error, if one failed.
    fn auto_save_done(&mut self) -> Option<String> {
        if self.ui.auto_saving.is_empty() {
            return None;
        }
        let mut failed = None;
        let done: Vec<_> = self.ui.auto_saving.extract_if(.., |(_, h)| h.is_finished()).collect();
        for (_, h) in done {
            let saved = match h.join() {
                Ok((snapshot, Ok(bytes))) => snapshot.save(&mut self.session, &bytes).map(|_| ()).map_err(|e| e.to_string()),
                Ok((_, Err(e))) => Err(e.to_string()),
                Err(_) => Err("the file couldn't be encoded".to_string()),
            };
            if let Err(e) = saved {
                failed.get_or_insert(e);
            }
        }
        failed
    }

    /// Draw the whole UI: the main window (the other windows draw themselves).
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        #[cfg(not(target_arch = "wasm32"))]
        let t0 = std::time::Instant::now();
        let ctx = ui.ctx().clone();
        self.ui_context = Some(ctx.clone());
        // `App::logic` already ticked this pass; direct harnesses do not call it. The marker only
        // covers the tick that shares this pass: clearing it here keeps a direct `App::ui` harness
        // ticking on every frame (it used to tick every other frame).
        if !std::mem::take(&mut self.logic_ticked) {
            self.background_tick(&ctx);
            self.logic_ticked = false;
        }
        // Setup once per frame (pass 0), not once per pass and never on a logic-only tick.
        let stamp = (ctx.viewport_id(), ctx.cumulative_frame_nr());
        let first = !self.fonts_installed;
        if ctx.current_pass_index() == 0 && self.ui_frame_stamp != Some(stamp) {
            self.ui_frame_stamp = Some(stamp);
            self.frame_stats.record(&ctx);
            self.ui_setup(&ctx);
        }
        if first && self.fonts_installed {
            // New fonts apply from the next frame; draw nothing until then. Anything taken into
            // this frame's (undrawn) input waits for the first drawn frame.
            ctx.request_repaint();
            if let Some(events) = self.applied_input.take() {
                self.push_control_input(events);
            }
            self.input_acks.append(&mut self.applied_acks);
            self.applied_frame = None;
            self.control_synthetic = false;
            // The next (drawn) frame ticks for itself in a direct harness.
            self.logic_ticked = false;
            self.observe_ime(&ctx, false);
            return;
        }
        windows::begin(self, &ctx);
        let (files, prefs) = (self.session.docs.len(), self.session.prefs.clone());
        self.frame(ui);
        windows::settle(self, &ctx, files);
        if self.session.prefs != prefs {
            windows::repaint_others(self, &ctx);
        }
        windows::show(self, &ctx);
        // The injected batch was applied by the frame that took it. A discarded sizing pass keeps
        // its acknowledgement for the presented pass but never replays the input (its side effects
        // already happened and would otherwise duplicate).
        self.end_input_frame(&ctx);
        // Plugins installed, removed or set up are kept at once: a crash doesn't lose them.
        // So are the owner tokens of files put online.
        if std::mem::take(&mut self.session.plugins.unsaved) | std::mem::take(&mut self.session.live.unsaved) {
            self.services.save_prefs(&self.session.prefs);
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.ui.frame_ms = t0.elapsed().as_secs_f64() * 1000.0;
        }
    }

    /// Whether the right panel shows: never with the UI `hidden`; always on a design file; on a
    /// board, which has no prototypes or Dev Mode (its tab stays Design), only when asked for.
    fn right_panel_shown(&mut self, hidden: bool) -> bool {
        let board = self.session.doc().is_ok_and(figcraft_engine::board::is_board);
        if board {
            self.ui.right_tab = RightTab::Design;
        }
        !hidden && (!board || self.ui.board_panel)
    }

    /// A narrow window's two buttons for the side panels (touch-sized); the open one closes again.
    fn mobile_buttons(&mut self, ctx: &egui::Context, at: egui::Pos2) {
        use a11y::Tip as _;
        let t = theme::Tokens::get(ctx);
        egui::Area::new(egui::Id::unique("mobile_panel_buttons")).order(egui::Order::Foreground).fixed_pos(at).show(ctx, |ui| {
            for (side, icon, name) in
                [(MobilePanel::Left, icons::Icon::Layers, i18n::tr("Layers")), (MobilePanel::Right, icons::Icon::Panels, i18n::tr("Design"))]
            {
                let open = self.ui.mobile_panel == Some(side);
                let (r, resp) = ui.allocate_exact_size(egui::vec2(40.0, 40.0), egui::Sense::click());
                let resp = resp.tip(name);
                let fill = if open { t.accent } else { t.panel };
                ui.painter().rect(r, 8.0, fill, egui::Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
                icons::paint(ui.painter(), r.shrink(10.0), icon, if open { t.accent_on } else { t.icon });
                if resp.clicked() {
                    if side == MobilePanel::Left && !open {
                        self.ui.open_left_panel();
                    } else {
                        self.ui.mobile_panel = if open { None } else { Some(side) };
                    }
                }
                ui.add_space(4.0);
            }
        });
    }

    /// One frame of the window drawing now.
    pub(crate) fn frame(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        // Bind this input batch before controls or pointer input can replace its composing
        // target/caret. Outgoing IME receipts run after mutation and cannot authorize it.
        let ime_input = shortcuts::ImeInputBaseline::capture(self);
        let t = theme::Tokens::get(&ctx);
        // A field that had the keyboard when the frame began keeps its keys from the shortcuts
        // even if it lets go during the frame (Enter commits it), or as it began (egui drops the
        // focus on Escape before the frame, which cancels the field's edit and nothing else).
        let typing = std::mem::take(&mut self.field_focus) || a11y::field_focused(&ctx);
        // Bind the originating field/file before tabs can switch. Its widget commits only after
        // processing this batch; delivery remains deferred until the following frame.
        let field_save = shortcuts::prepare_field_save(self, &ctx);
        let swallowed = shortcuts::swallowed_paste(self, &ctx);
        // A dialog that closed acts first, in the window it was opened from.
        self.poll_dialog(&ctx);
        // Control requests run first so their results reflect this frame.
        control::drain(self, &ctx);
        windows::requests(self, &ctx);
        selection_spacing::sync(self, &ctx);
        // Before any widget: the navigation keys (F6, Tab, arrows, Escape) move the keyboard.
        a11y::begin(self, &ctx, ui.scope_id());
        if self.ui.title_bar == TitleBar::Custom {
            tabs::resize_edges(&ctx);
        }
        if self.ui.icon_gallery {
            icons::gallery(&ctx);
        }
        theme::set_doc_profile(&ctx, self.session.doc().map_or(figcraft_color::ColorProfile::Srgb, |d| d.color_profile));
        widgets::set_nudge(&ctx, self.session.prefs.nudge_small, self.session.prefs.nudge_big);
        self.sync_dev_mode();
        self.end_pen_outside_design();
        tabs::relink_presents(self);
        tabs::tab_bar(self, ui);
        // Control/window work and this pass's physical tab click may have replaced the owner.
        // A shared field ID in that replacement must never consume the old commit marker.
        let field_save = shortcuts::revalidate_field_save(self, &ctx, field_save);
        if self.ui.present.is_some() {
            // A close request can arrive while the presenter fills the window. Let its unsaved
            // changes dialog take Escape and draw above the player just as it does in the editor.
            dialogs::keys(self, &ctx);
            present::show(self, ui);
            dialogs::show(self, &ctx);
            updates_ui::show(self, &ctx);
            // What went wrong while presenting (a widget's plugin failing) shows over it.
            self.toast_ui(&ctx);
            windows::requests(self, &ctx);
            shortcuts::clear_field_save_marker(&ctx);
            self.observe_ime(&ctx, false);
            return;
        }
        if !self.has_files() || self.ui.home {
            home::show(self, ui);
            // The shortcuts panel belongs to the canvas; it doesn't hang over the home screen
            // (cleared after the home widgets, which may have just asked for it).
            self.ui.shortcuts_panel = false;
            dialogs::keys(self, &ctx);
            dialogs::show(self, &ctx);
            updates_ui::show(self, &ctx);
            self.toast_ui(&ctx);
            windows::requests(self, &ctx);
            shortcuts::clear_field_save_marker(&ctx);
            self.observe_ime(&ctx, false);
            return;
        }
        let hidden = self.session.ds().map(|d| d.view.ui_hidden).unwrap_or(false);
        let narrow = !hidden && ui.available_width() < NARROW_WIDTH;
        let mobile = self.ui.mobile_panel.filter(|_| narrow);
        self.ui.narrow = narrow;
        // A narrow window shows the right panel when its button asks for it, a board's too.
        let right_open = (self.right_panel_shown(hidden) && !narrow) || mobile == Some(MobilePanel::Right);
        if !hidden && (!narrow || mobile == Some(MobilePanel::Left)) {
            egui::Panel::left("left_rail")
                .exact_size(56.0)
                .resizable(false)
                .frame(egui::Frame::NONE.fill(t.panel).stroke(egui::Stroke::new(1.0, t.border)))
                .show(ui, |ui| {
                    a11y::region(&mut self.ui.nav, ui, a11y::Region::Layers, egui::accesskit::Role::Navigation, i18n::tr("Sidebar"));
                    layers::rail(self, ui)
                });
        }
        // The panels are as wide as asked, less in a window too small for them and the canvas.
        let left_open = !hidden && if narrow { mobile == Some(MobilePanel::Left) } else { !self.ui.left_collapsed };
        let (left_w, right_w) = if narrow {
            let w = (ui.available_width() - MOBILE_STRIP).max(0.0);
            (if left_open { w } else { 0.0 }, w)
        } else {
            fit_panels(ui.available_width(), left_open.then_some(self.ui.left_width), right_open.then_some(self.ui.right_width))
        };
        let rows = ui.available_rect_before_wrap().y_range();
        let mut edges: Vec<(&str, f32, f32, f32)> = Vec::new();
        if left_open {
            if !narrow {
                edges.push(("left", ui.available_rect_before_wrap().min.x + left_w, left_w, 1.0));
            }
            egui::Panel::left("left_panel")
                .resizable(false)
                .exact_size(left_w)
                .frame(egui::Frame::NONE.fill(t.panel).stroke(egui::Stroke::new(1.0, t.border)))
                .show(ui, |ui| {
                    let label = match self.ui.left_tab {
                        _ if self.ui.versions.open => i18n::tr("Version history"),
                        LeftTab::File => i18n::tr("Layers"),
                        LeftTab::Assets => i18n::tr("Assets"),
                    };
                    a11y::region(&mut self.ui.nav, ui, a11y::Region::Layers, egui::accesskit::Role::Pane, label);
                    layers::left_panel(self, ui)
                });
        }
        if right_open {
            if !narrow {
                edges.push(("right", ui.available_rect_before_wrap().max.x - right_w, right_w, -1.0));
            }
            egui::Panel::right("right_panel")
                .resizable(false)
                .exact_size(right_w)
                .frame(egui::Frame::NONE.fill(t.panel).stroke(egui::Stroke::new(1.0, t.border)))
                .show(ui, |ui| {
                    let label = match self.ui.right_tab {
                        RightTab::Design => i18n::tr("Design"),
                        RightTab::Prototype => i18n::tr("Prototype"),
                        RightTab::Inspect => i18n::tr("Dev Mode"),
                    };
                    a11y::region(&mut self.ui.nav, ui, a11y::Region::Properties, egui::accesskit::Role::Pane, label);
                    right::right_panel(self, ui)
                });
        }
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(t.canvas)).show(ui, |ui| {
            let worker = self.worker.take();
            let mut worker = worker.unwrap_or_else(|| render_worker::Worker::new(ctx.clone()));
            canvas::show(self, ui, &mut worker);
            self.worker = Some(worker);
            if !hidden && mobile.is_none() {
                ui.scope_builder(egui::UiBuilder::new().id_salt("toolbar"), |ui| {
                    a11y::container(ui, egui::accesskit::Role::Toolbar, i18n::tr("Tools"));
                    toolbar::show(self, ui);
                });
            }
        });
        if narrow {
            let x = if mobile == Some(MobilePanel::Left) { ctx.content_rect().max.x - MOBILE_STRIP + 4.0 } else { ctx.content_rect().min.x + 4.0 };
            self.mobile_buttons(&ctx, egui::pos2(x, rows.min + 8.0));
        }
        // The panels' edges, over the canvas and the panels (drawn after them: they take the press).
        for (name, x, shown, grows) in edges {
            if let Some(w) = Self::panel_edge(ui, name, x, rows, shown, grows) {
                let (l, r) = if name == "left" { (w, self.ui.right_width) } else { (self.ui.left_width, w) };
                self.set_panel_widths(l, r);
                self.panels_unsaved = true;
            }
        }
        // Kept once the drag is over (not on every pointer move).
        if self.panels_unsaved && !ctx.input(|i| i.pointer.any_down()) {
            self.panels_unsaved = false;
            self.services.save_prefs(&self.session.prefs);
        }
        // Before the shortcuts: keys the hovered pane's prototype uses don't reach the editor, and
        // the arrows step through comment threads with the Comment tool.
        present::show_pane(self, &ctx);
        comments::keys(self, &ctx);
        dev_compare::keys(self, &ctx);
        branch_ui::keys(self, &ctx);
        live::dialog_keys(self, &ctx);
        dialogs::keys(self, &ctx);
        shortcuts::handle_input(self, &ctx, typing, swallowed, ime_input, field_save);
        menus::popups(self, &ctx);
        variable_ui::picker(self, &ctx);
        style_picker::show(self, &ctx);
        self.end_unwanted_text_preview(&ctx);
        self.end_pen_outside_design();
        dialogs::show(self, &ctx);
        updates_ui::show(self, &ctx);
        self.toast_ui(&ctx);
        // A rename started after the layers panel drew (Cmd/Ctrl+R, a double-click) opens its field
        // next frame.
        if self.ui.rename.is_some_and(|r| r.start) && !hidden && !self.ui.left_collapsed && self.ui.left_tab == LeftTab::File {
            ctx.request_repaint();
        }
        a11y::end(self, &ctx);
        self.field_focus = a11y::field_focused(&ctx);
        windows::requests(self, &ctx);
        // Your pointer, selection and view to the others in each live session (when they
        // changed), from the window with its file.
        let windows = &self.windows;
        self.session.live_sync_in(|uid| uid.map_or(0, |u| windows.of(u)) == windows.current);
        // The canvas editor's IME context is observed exactly once per pass, after every command
        // this pass ran (shortcuts, menus, dialogs, window requests and live sync). A focused panel
        // field or an open palette owns the platform IME instead; the early `home`/present paths
        // above make the same single idle observation.
        self.observe_ime(&ctx, !typing && !self.field_focus && self.ui.palette.is_none());
    }

    /// The pass's one IME observation: the focused on-canvas editor, or an explicit idle.
    ///
    /// Calling `ImeContextTracker::observe` more than once in a pass would treat an idle followed
    /// by a focused observation as a context change (dropping the frame's receipts) and bump the
    /// generation. Keeping this in one place makes the fork contract structural.
    fn observe_ime(&mut self, ctx: &egui::Context, focused: bool) {
        #[cfg(test)]
        {
            self.ime_observations = self.ime_observations.saturating_add(1);
        }
        if focused {
            shortcuts::ime_output(self, ctx);
        } else {
            let _ = self.ime.observe(ctx.viewport_id(), None);
        }
    }

    /// Draw the toast: start its lifetime on the frame **after** the first drawn frame, request
    /// that frame and the frame that hides it, expire and remove it at the deadline, and place it
    /// at the bottom centre.
    fn toast_ui(&mut self, ctx: &egui::Context) {
        let Some((msg, clock)) = &mut self.ui.toast else { return };
        let now = ctx.input(|i| i.time);
        // The clock starts on the frame after the one that first draws the toast: that frame's
        // time was taken before whatever raised it ran (a file dialog, a long open), and would
        // count that wait against it, so a failed open's message expired before it was seen.
        let left = match *clock {
            ToastClock::Raised => {
                *clock = ToastClock::Drawn;
                ctx.request_repaint();
                TOAST_SECS
            }
            ToastClock::Drawn => {
                *clock = ToastClock::Since(now);
                TOAST_SECS
            }
            ToastClock::Since(t) => TOAST_SECS - (now - t),
        };
        if left <= 0.0 {
            self.ui.toast = None;
            return;
        }
        let msg = msg.clone();
        // Nothing moves while it shows: the next frame it needs is the one that hides it.
        if matches!(clock, ToastClock::Since(_)) {
            repaint_in(ctx, left);
        }
        let screen = ctx.content_rect();
        egui::Area::new(egui::Id::unique("toast"))
            .fixed_pos(egui::pos2(screen.center().x, screen.max.y - 96.0))
            .pivot(egui::Align2::CENTER_BOTTOM)
            .order(egui::Order::Tooltip)
            .show(ctx, |ui| {
                egui::Frame::NONE
                    .fill(egui::Color32::from_rgb(0x1E, 0x1E, 0x1E))
                    .corner_radius(egui::CornerRadius::same(8))
                    .inner_margin(egui::Margin::symmetric(14, 10))
                    .show(ui, |ui| {
                        // Wrapped at a readable width (the area alone would wrap at a sliver).
                        let width = (screen.width() - 64.0).clamp(120.0, 480.0);
                        let galley = ui.painter().layout(msg, theme::regular(12.0), egui::Color32::WHITE, width);
                        ui.label(galley);
                    });
            });
    }

    /// UI state as JSON (control channel `ui.inspect`).
    pub fn inspect(&self) -> Value {
        let mut state = json!({
            "rightTab": self.ui.right_tab,
            "leftTab": self.ui.left_tab,
            "home": self.ui.home,
            "online": online::inspect(self),
            "tabs": self.session.docs.iter().map(|d| json!({"name": d.doc.name, "live": tabs::live_mark(self, d.uid).map(|shared| if shared { "shared" } else { "private" })})).collect::<Vec<_>>(),
            "homeSection": self.ui.home_view.section.id(),
            "homeRecents": home_recents::inspect(self),
            "dark": self.ui.dark,
            "focus": {"region": self.ui.nav.region.id(), "keyboard": self.ui.nav.keyboard},
            "reduceMotion": self.reduce_motion(),
            "presenting": self.ui.present.is_some(),
            "presenter": tabs::shown_or_paused(self).map(present::Presenter::inspect),
            "presentOptions": {"scale": self.ui.present_view.scale.id(), "deviceFrame": self.ui.present_view.device_frame, "comments": self.ui.present_view.comments, "hotspotHints": self.ui.present_view.hotspot_hints, "inspect": self.ui.present_view.inspect},
            "preview": self.ui.preview.as_ref().map(|p| p.player.inspect()),
            "mainMenu": self.ui.main_menu,
            "palette": self.ui.palette.is_some(),
            "variables": self.ui.variables.is_some(),
            "versionHistory": self.ui.versions.open,
            "branchReview": self.ui.branch_review.as_ref().map(|r| json!({"id": r.id, "picks": r.picks, "mode": if r.mode == dev_compare::Mode::Overlay { "overlay" } else { "sideBySide" }})),
            "devCompare": self.ui.dev.compare.as_ref().map(|c| json!({"id": c.id, "mode": if c.mode == dev_compare::Mode::Overlay { "overlay" } else { "sideBySide" }, "split": c.split, "version": c.version})),
            "devFocus": self.session.ds().ok().and_then(figcraft_engine::DocState::focused),
            "annotationFilter": self.ui.dev.categories.id(),
            "colorProfileDialog": self.ui.color_profile.map(|p| p.id()),
            "stylePicker": self.ui.style_picker.as_ref().map(|p| p.slot().key()),
            "stylePickerView": if self.ui.style_grid { "grid" } else { "list" },
            "textPreview": figcraft_engine::textpreview::active(&self.session),
            "comments": {
                "open": self.ui.comments.open,
                "draft": self.ui.comments.draft.as_ref().map(|d| [d.0, d.1]),
                "filter": self.ui.comments.filter.id(),
                "sort": self.ui.comments.sort.id(),
                "search": self.ui.comments.search,
                "editing": self.ui.comments.editing.as_ref().map(|e| [e.0, e.1 as u64]),
                "mentions": self.ui.comments.mention.names(),
            },
            "frameMs": self.ui.frame_ms,
            "renderMs": self.ui.render_ms,
            "framesDrawn": self.frame_stats.frames,
            "repaintCauses": self.frame_stats.causes(),
            "canvas": {
                "rect": [self.canvas.rect.min.x, self.canvas.rect.min.y, self.canvas.rect.max.x, self.canvas.rect.max.y],
                "renderWork": self.canvas.render_work.map(|w| match w {
                    figcraft_render::Work::Reused => json!({"kind": "reused"}),
                    figcraft_render::Work::Partial { pixels } => json!({"kind": "partial", "pixels": pixels}),
                    figcraft_render::Work::Full => json!({"kind": "full"}),
                }),
                "quality": if self.canvas.tex_coarse { "preview" } else { "refined" },
                "previewLatencyMs": self.canvas.preview_latency_ms,
                "refineLatencyMs": self.canvas.refine_latency_ms,
            },
            "live": live::inspect(self),
            "window": self.windows.current,
            "windows": windows::list(self),
            "tool": self.session.tool_info(),
            "cursor": self.canvas.cursor.map(cursors::Shown::name),
            "panels": {"left": self.ui.left_width, "right": self.ui.right_width, "leftCollapsed": self.ui.left_collapsed},
            "status": self.session.status(),
            "lastError": self.ui.last_error,
            "importReport": self.ui.import_report.as_ref().map(import_report::Report::inspect),
        });
        state["homeSearch"] = json!(self.ui.home_view.file_query);
        // Set apart from the literal above, which is at the macro recursion limit.
        state["uiScale"] = json!({"choice": self.session.prefs.ui_scale, "zoom": self.ui.ui_scale.zoom, "drawn": self.ui.ui_scale.drawn, "limited": self.ui.ui_scale.limited});
        state["pendingOpens"] = json!(self.ui.pending_opens.len());
        state["missingFonts"] = json!(self.ui.missing_fonts.as_ref().map(missing_fonts::Shown::inspect));
        state["presentTabs"] = tabs::present_tabs_inspect(self);
        state
    }
}
