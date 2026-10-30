//! Stable shared-budget dialog layout and its cached, explicitly expanded comparison.
use super::*;

/// A conflict is immutable until it is replaced or resolved. Decrypt once, and only build the
/// complete comparison strings when the user expands them. Neither operation belongs in repaint.
pub(super) struct ConflictPreview {
    id: String,
    revision: u64,
    snapshot: Result<budget_core::sharing::Snapshot, String>,
    complete: Option<Vec<String>>,
}

impl ConflictPreview {
    fn new(remote: &Remote, key: &str) -> Self {
        Self { id: remote.id.clone(), revision: remote.revision,
            snapshot: client::unlock(remote, key), complete: None }
    }

    fn matches(&self, remote: &Remote) -> bool { self.id == remote.id && self.revision == remote.revision }

    fn complete(&mut self, local: &budget_core::sharing::Snapshot) -> Option<&[String]> {
        if self.complete.is_none() {
            if let Ok(remote) = &self.snapshot {
                let mut rows = vec!["Your local version".into()];
                for (index, snapshot) in [local, remote].into_iter().enumerate() {
                    if index == 1 { rows.push(String::new()); rows.push("Current shared version".into()); }
                    let json = serde_json::to_string_pretty(snapshot).unwrap_or_default();
                    for line in json.lines() {
                        // A note can occupy megabytes on one JSON line. Bound each displayed row
                        // as well as virtualizing the row count, so repaint work stays bounded.
                        let mut start = 0;
                        for (count, (offset, _)) in line.char_indices().enumerate() {
                            if count != 0 && count % 256 == 0 {
                                rows.push(line[start..offset].to_owned()); start = offset;
                            }
                        }
                        rows.push(line[start..].to_owned());
                    }
                }
                self.complete = Some(rows);
            }
        }
        self.complete.as_deref()
    }
}

impl App {
    pub(super) fn shared_panel_contents(&mut self, ctx: &egui::Context) -> Option<&'static str> {
        let mut command = None;
        let mut recover = None;
        let busy = self.sharing.job.is_some();
        if let Some(doc) = &self.sharing.document {
            if let Some(remote) = &doc.conflict {
                if self.sharing.preview.as_ref().is_none_or(|preview| !preview.matches(remote)) {
                    self.sharing.preview = Some(ConflictPreview::new(remote, &doc.recovery_key));
                }
            } else { self.sharing.preview = None; }
        } else { self.sharing.preview = None; }

        let t = self.t.clone();
        let frame = t.card();
        let bounds = ctx.screen_rect().shrink(16.0);
        let outer_size = Vec2::new(564.0_f32.min(bounds.width()), 684.0_f32.min(bounds.height()));
        let content_size = (outer_size - frame.total_margin().sum()).max(Vec2::splat(1.0));
        let id = egui::Id::new("shared-budget");
        egui::Modal::new(id)
            .area(egui::Modal::default_area(id).default_size(outer_size).constrain_to(bounds))
            .frame(frame)
            .show(ctx, |ui| {
                // An Area supplies the previous frame's size to its contents. Set both dimensions
                // before scrolling so status changes and a smaller previous viewport cannot feed
                // back into the dialog's size.
                ui.set_width(content_size.x);
                ui.set_height(content_size.y);
                ui.heading("Shared budget");
                ui.add(egui::Label::new(egui::RichText::new("Encrypted household sync. Your recovery key stays on your devices.")
                    .color(t.text_muted)).truncate())
                    .on_hover_text("Encrypted household sync. Your recovery key stays on your devices.");
                let status = self.sharing.status(self.dirty).unwrap_or_else(|| "Sign in or open a saved household.".into());
                let status = if busy { format!("Connecting… · {status}") } else { status };
                // A static status has no animation repaint loop and always occupies the same row.
                ui.add_sized([content_size.x, 24.0], egui::Label::new(status).truncate());

                let body_height = (ui.available_height() - 58.0).max(1.0);
                ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
                egui::ScrollArea::vertical().id_salt("shared_budget_body")
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
                    .auto_shrink([false, false]).min_scrolled_height(0.0)
                    .max_height(body_height).show(ui, |ui| {
                        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                        if let Some(doc) = &self.sharing.document {
                            ui.label(egui::RichText::new(format!("{} · revision {} · {}", doc.working.name, doc.revision, doc.role))
                                .text_style(ts(SEMI)));
                            ui.label(format!("Household time zone: {}", doc.working.time_zone));
                            if ui.button("Copy recovery key").on_hover_text("Copy this household's key to unlock it on another device. Keep it private.").clicked() {
                                command = Some("copy-key");
                            }
                            if ui.button("Sync now").clicked() { command = Some("sync"); }
                            if let Ok(server) = client::normalize_server(&doc.connection.server) {
                                ui.hyperlink_to("Open mobile app · invitations, members and history", server);
                            }
                            if let Some(remote) = &doc.conflict {
                                ui.separator(); ui.heading("Review both versions");
                                ui.label(format!("Your device: {} payments, {} queued changes", doc.working.payments.len(), doc.pending.len()));
                                if let Some(preview) = &mut self.sharing.preview {
                                    match &preview.snapshot {
                                        Ok(snapshot) => {
                                            ui.label(format!("Current shared: {} payments · {} · {}", snapshot.payments.len(), remote.updated_at, remote.summary));
                                            egui::CollapsingHeader::new("Compare complete contents").show(ui, |ui| {
                                                if let Some(rows) = preview.complete(&doc.working) {
                                                    ui.label(egui::RichText::new("Long lines continue on the next row.").color(t.text_muted));
                                                    ui.scope(|ui| {
                                                        ui.spacing_mut().item_spacing.y = 0.0;
                                                        let height = ui.text_style_height(&egui::TextStyle::Monospace);
                                                        egui::ScrollArea::both().id_salt("shared_conflict_contents")
                                                            .auto_shrink([false, false]).min_scrolled_height(0.0)
                                                            .max_height(200.0).show_rows(ui, height, rows.len(), |ui, range| {
                                                                for row in &rows[range] {
                                                                    ui.add(egui::Label::new(egui::RichText::new(row).monospace()).extend());
                                                                }
                                                            });
                                                    });
                                                }
                                            });
                                        }
                                        Err(error) => { ui.colored_label(t.negative, error); }
                                    }
                                }
                                ui.label("Keep mine reapplies only records and settings changed on this device to the current shared version.");
                                if ui.button("Export local backup…").clicked() { command = Some("export"); }
                                if let Some(count) = self.sharing.confirm_discard {
                                    let changes = changes_text(count);
                                    ui.label(egui::RichText::new(format!("Discard {changes}?")).text_style(ts(SEMI)));
                                    ui.label("The current shared version will replace what is on this device. A protected copy stays on this device, and you can get your changes back here.");
                                    ui.horizontal_wrapped(|ui| {
                                        if theme::danger_button(ui, format!("Discard {changes}"), &t).clicked() { command = Some("discard"); }
                                        if theme::secondary_button(ui, "Keep my changes", &t).clicked() { command = Some("keep-discard"); }
                                    });
                                } else {
                                    ui.horizontal_wrapped(|ui| {
                                        if ui.button("Keep mine (reapply my changes)").clicked() { command = Some("mine"); }
                                        if ui.button("Use current shared version").clicked() { command = Some("theirs"); }
                                    });
                                }
                            }
                            if !self.sharing.discarded.is_empty() {
                                ui.separator(); ui.heading("Changes you discarded");
                                for (index, copy) in self.sharing.discarded.iter().enumerate() {
                                    ui.label(format!("{}, discarded {}", changes_text(copy.changes), copy.discarded_at));
                                    let back = ui.add_enabled(doc.conflict.is_none(), Button::new("Get these changes back"))
                                        .on_hover_text("Adds them to the current shared version as new changes.")
                                        .on_disabled_hover_text("Resolve the conflict first.");
                                    if back.clicked() { command = Some("recover"); recover = Some(index); }
                                }
                            }
                        }

                        ui.separator();
                        egui::CollapsingHeader::new("Sign in / open another household")
                            .default_open(self.sharing.document.is_none()).show(ui, |ui| {
                                ui.label("Server address");
                                ui.add(egui::TextEdit::singleline(&mut self.sharing.server).desired_width(ui.available_width()));
                                ui.label("Username");
                                ui.add(egui::TextEdit::singleline(&mut self.sharing.username).desired_width(ui.available_width()));
                                ui.label("Password");
                                ui.add(egui::TextEdit::singleline(&mut self.sharing.password).password(true).desired_width(ui.available_width()));
                                if ui.add_enabled(!busy, Button::new("Sign in / refresh session")).clicked() { command = Some("login"); }
                                if let Some(connection) = &self.sharing.connection {
                                    if let Ok(server) = client::normalize_server(&connection.server) {
                                        ui.hyperlink_to("Open mobile app to set up an account or accept an invitation", server);
                                    }
                                    ui.separator();
                                    ui.label("Recovery key for the household you want to open");
                                    ui.add(egui::TextEdit::singleline(&mut self.sharing.key).password(true).desired_width(ui.available_width()));
                                    for budget in &self.sharing.budgets {
                                        let label = format!("Open {} · revision {}", budget["name"].as_str().unwrap_or("Household"), budget["revision"]);
                                        if ui.add_enabled(!busy && !self.sharing.key.is_empty(), Button::new(label)).clicked() {
                                            command = Some("open"); self.sharing.name = budget["id"].as_str().unwrap_or_default().into();
                                        }
                                    }
                                    if self.db.is_some() && self.sharing.document.is_none() {
                                        ui.separator();
                                        if ui.button("Prepare to share current budget…").clicked() { command = Some("prepare"); }
                                        if self.sharing.creating {
                                            ui.label("Household name");
                                            ui.add(egui::TextEdit::singleline(&mut self.sharing.name).desired_width(ui.available_width()));
                                            ui.label("Household time zone (IANA, for example America/Halifax)");
                                            ui.add(egui::TextEdit::singleline(&mut self.sharing.zone).desired_width(ui.available_width()));
                                            ui.label("Save this recovery key somewhere safe. Share it with household members separately from invitations.");
                                            ui.add(egui::TextEdit::multiline(&mut self.sharing.key).desired_rows(2).desired_width(ui.available_width()));
                                            if ui.button("Copy recovery key").clicked() { ui.ctx().copy_text(self.sharing.key.clone()); }
                                            ui.checkbox(&mut self.sharing.saved_key, "I saved the recovery key");
                                            if ui.add_enabled(!busy && self.sharing.saved_key && !self.has_draft(), Button::new("Share current budget (upload encrypted copy)")).clicked() {
                                                command = Some("create");
                                            }
                                        }
                                    }
                                } else if let Ok(server) = client::normalize_server(&self.sharing.server) {
                                    ui.hyperlink_to("Open mobile app for account setup / invitation", server);
                                }
                            });
                        ui.separator();
                        if ui.add_enabled(!busy, Button::new("Open cached shared budget on this device")).clicked() { command = Some("cache"); }
                        if ui.add_enabled(!busy, Button::new("Resume pending household creation")).clicked() { command = Some("resume-create"); }
                        if let Some(error) = &self.sharing.error {
                            ui.separator();
                            ui.colored_label(t.negative, error);
                        }
                    });
                ui.separator();
                if theme::secondary_button(ui, "Close", &t).clicked() || ui.input(|i| i.key_pressed(Key::Escape)) {
                    self.sharing.open = false;
                }
            });
        self.sharing.recover = recover;
        command
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (App, PathBuf) {
        let (mut app, dir) = crate::lifecycle_tests::fixture();
        app.sharing.cache = dir.join("shared.enc");
        let snapshot = snapshot_from_db_with_operation(app.db.as_ref().unwrap(),
            &uuid::Uuid::new_v4().to_string(), "Fixture household", "America/Halifax", None,
            &uuid::Uuid::new_v4().to_string()).unwrap();
        let key = client::generate_recovery_key();
        let connection = Connection { server: "http://127.0.0.1:8787".into(), token: "fixture-session".into(),
            user: client::User { id: "fixture".into(), username: "fixture".into() } };
        let remote = Remote { id: snapshot.id.clone(), name: snapshot.name.clone(), revision: 1,
            payload: client::seal(&snapshot, &key, 1).unwrap(), updated_at: "fixture-1".into(),
            role: "owner".into(), summary: "Fixture update".into() };
        app.sharing.document = Some(SharedDocument::open(connection.clone(), key, remote).unwrap());
        app.sharing.connection = Some(connection);
        app.sharing.open = true;
        app.dirty = false;
        (app, dir)
    }

    fn context(app: &App) -> egui::Context {
        let ctx = egui::Context::default();
        theme::install_fonts(&ctx);
        theme::apply(&ctx, &app.t);
        ctx
    }

    fn frame(ctx: &egui::Context, app: &mut App, size: Vec2, scale: f32, events: Vec<egui::Event>) -> egui::FullOutput {
        let mut input = egui::RawInput { time: Some(ctx.input(|i| i.time) + 0.1),
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)), events, ..Default::default() };
        input.viewports.get_mut(&egui::ViewportId::ROOT).unwrap().native_pixels_per_point = Some(scale);
        ctx.run(input, |ctx| app.shared_panel(ctx))
    }

    fn visible_text(output: &egui::FullOutput, label: &str) -> Option<egui::Rect> {
        fn find(shape: &egui::Shape, clip: egui::Rect, label: &str) -> Option<egui::Rect> {
            match shape {
                egui::Shape::Text(text) if text.galley.job.text == label => {
                    let rect = text.galley.mesh_bounds.translate(text.pos.to_vec2());
                    clip.expand(1.0).contains_rect(rect).then_some(rect)
                }
                egui::Shape::Vec(shapes) => shapes.iter().find_map(|shape| find(shape, clip, label)),
                _ => None,
            }
        }
        output.shapes.iter().find_map(|shape| find(&shape.shape, shape.clip_rect, label))
    }

    fn settled(ctx: &egui::Context, app: &mut App, size: Vec2, scale: f32) -> egui::FullOutput {
        let mut output = frame(ctx, app, size, scale, vec![]);
        for _ in 0..3 { output = frame(ctx, app, size, scale, vec![]); }
        output
    }

    fn click(ctx: &egui::Context, app: &mut App, size: Vec2, pos: egui::Pos2) {
        for pressed in [true, false] {
            frame(ctx, app, size, 1.0, vec![egui::Event::PointerMoved(pos), egui::Event::PointerButton {
                pos, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE }]);
        }
    }

    fn scroll_to(ctx: &egui::Context, app: &mut App, size: Vec2, label: &str) -> egui::FullOutput {
        for _ in 0..100 {
            let output = settled(ctx, app, size, 1.0);
            if visible_text(&output, label).is_some() { return output; }
            let modal = ctx.memory(|m| m.area_rect(egui::Id::new("shared-budget"))).unwrap();
            // The left edge of the body is outside the indented conflict comparison's nested
            // scroll area, so this exercises the outer scroll as a user can with its scrollbar.
            let pos = egui::pos2(modal.min.x + 20.0, modal.min.y + 140.0);
            frame(ctx, app, size, 1.0, vec![egui::Event::PointerMoved(pos), egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point, delta: Vec2::new(0.0, -80.0), modifiers: Modifiers::NONE }]);
        }
        panic!("{label} must be reachable by scrolling");
    }

    #[test]
    fn polling_and_errors_do_not_move_household_actions() {
        let (mut app, dir) = fixture();
        let ctx = context(&app);
        let size = Vec2::new(900.0, 600.0);
        let output = settled(&ctx, &mut app, size, 1.0);
        let before: Vec<_> = ["Copy recovery key", "Sync now"].map(|label|
            visible_text(&output, label).unwrap_or_else(|| panic!("{label} must be visible"))).into();
        let (send, recv) = mpsc::channel();
        app.sharing.job = Some(recv);
        for phase in 0..4 {
            if phase == 1 { app.sharing.error = Some("The server is unavailable. Your queued changes remain protected on this device. ".repeat(30)); }
            if phase == 2 { app.sharing.job = None; }
            if phase == 3 { app.sharing.error = None; }
            let output = settled(&ctx, &mut app, size, 1.0);
            for (label, before) in ["Copy recovery key", "Sync now"].into_iter().zip(&before) {
                assert_eq!(visible_text(&output, label).as_ref(), Some(before), "{label} moved during phase {phase}");
            }
        }
        drop(send);
        drop(app); std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn compact_and_high_dpi_dialogs_fit_and_keep_close_visible() {
        for scale in [1.0, 1.25, 1.5, 2.0] {
            let (mut app, dir) = fixture();
            let ctx = context(&app);
            for physical in [Vec2::new(1200.0, 800.0), Vec2::new(900.0, 600.0), Vec2::new(720.0, 480.0)] {
                let size = physical / scale;
                let output = settled(&ctx, &mut app, size, scale);
                let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
                let modal = ctx.memory(|m| m.area_rect(egui::Id::new("shared-budget"))).unwrap();
                assert!(screen.expand(1.0).contains_rect(modal), "scale {scale}, viewport {size:?}, modal {modal:?}");
                assert!(visible_text(&output, "Close").is_some(), "Close must be visible at scale {scale}, {size:?}");
            }
            drop(app); std::fs::remove_dir_all(dir).unwrap();
        }
    }

    #[test]
    fn pinned_close_works_with_long_errors_and_escape_preserves_the_document() {
        let (mut app, dir) = fixture();
        let before = serde_json::to_value(app.sharing.document.as_ref().unwrap()).unwrap();
        app.sharing.error = Some("Fixture connection error. ".repeat(100));
        let ctx = context(&app); let size = Vec2::new(640.0, 400.0);
        let output = settled(&ctx, &mut app, size, 1.5);
        let pos = visible_text(&output, "Close").expect("long errors must not hide Close").center();
        for pressed in [true, false] {
            frame(&ctx, &mut app, size, 1.5, vec![egui::Event::PointerMoved(pos), egui::Event::PointerButton {
                pos, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE }]);
        }
        assert!(!app.sharing.open);
        app.sharing.open = true;
        settled(&ctx, &mut app, size, 1.5);
        frame(&ctx, &mut app, size, 1.5, vec![egui::Event::Key { key: Key::Escape,
            physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }]);
        assert!(!app.sharing.open);
        assert_eq!(serde_json::to_value(app.sharing.document.as_ref().unwrap()).unwrap(), before);
        drop(app); std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn background_job_status_does_not_create_an_animation_repaint_loop() {
        let (mut app, dir) = fixture();
        let (send, recv) = mpsc::channel(); app.sharing.job = Some(recv);
        let ctx = context(&app);
        let output = settled(&ctx, &mut app, Vec2::new(900.0, 600.0), 1.0);
        let repaint = output.viewport_output[&egui::ViewportId::ROOT].repaint_delay;
        assert!(repaint >= Duration::from_secs(1), "a static pending status must not request animation frames: {repaint:?}");
        drop(send); drop(app); std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn long_household_lists_and_creation_fields_remain_reachable_with_close_pinned() {
        for creation in [false, true] {
            let (mut app, dir) = fixture(); app.sharing.document = None;
            app.sharing.key = client::generate_recovery_key();
            app.sharing.creating = creation; app.sharing.saved_key = true;
            if !creation {
                app.sharing.budgets = (0..50).map(|index| json!({"id":format!("fixture-{index}"),
                    "name":format!("Household {index}"),"revision":1})).collect();
            }
            let ctx = context(&app); let size = Vec2::new(640.0, 400.0);
            let initial = settled(&ctx, &mut app, size, 1.0);
            assert!(visible_text(&initial, "Server address").is_some());
            let close = visible_text(&initial, "Close").unwrap();
            let label = if creation { "Share current budget (upload encrypted copy)" } else { "Open Household 49 · revision 1" };
            let output = scroll_to(&ctx, &mut app, size, label);
            assert_eq!(visible_text(&output, "Close"), Some(close));
            let output = scroll_to(&ctx, &mut app, size, "Resume pending household creation");
            assert_eq!(visible_text(&output, "Close"), Some(close));
            assert!(app.sharing.open);
            drop(app); std::fs::remove_dir_all(dir).unwrap();
        }
    }

    /// A real click on whatever button paints `label`, scrolling to it first.
    fn press(ctx: &egui::Context, app: &mut App, size: Vec2, label: &str) {
        let output = scroll_to(ctx, app, size, label);
        let pos = visible_text(&output, label).unwrap().center();
        click(ctx, app, size, pos);
    }

    #[test]
    fn using_the_shared_version_names_the_count_and_can_be_cancelled_or_undone() {
        let (mut app, dir) = fixture();
        let doc = app.sharing.document.as_mut().unwrap();
        let mut theirs = doc.working.clone(); theirs.notes = "Other device fixture".into();
        for note in ["First queued fixture", "Second queued fixture"] {
            let mut mine = doc.working.clone(); mine.notes = note.into(); doc.capture(mine).unwrap();
        }
        doc.conflict = Some(Remote { id: theirs.id.clone(), name: theirs.name.clone(), revision: 2,
            payload: client::seal(&theirs, &doc.recovery_key, 2).unwrap(), updated_at: "fixture-2".into(),
            role: "owner".into(), summary: "Fixture conflict".into() });
        let ctx = context(&app); let size = Vec2::new(900.0, 700.0);
        press(&ctx, &mut app, size, "Use current shared version");
        let output = settled(&ctx, &mut app, size, 1.0);
        assert!(visible_text(&output, "Discard 2 unsynced changes?").is_some());
        assert_eq!(app.sharing.document.as_ref().unwrap().pending.len(), 2, "the first click only asks");

        press(&ctx, &mut app, size, "Keep my changes");
        let output = settled(&ctx, &mut app, size, 1.0);
        assert!(visible_text(&output, "Discard 2 unsynced changes?").is_none());
        assert_eq!(app.sharing.document.as_ref().unwrap().pending.len(), 2);

        press(&ctx, &mut app, size, "Use current shared version");
        press(&ctx, &mut app, size, "Discard 2 unsynced changes");
        let doc = app.sharing.document.as_ref().unwrap();
        assert!(doc.conflict.is_none() && doc.pending.is_empty(), "{:?}", app.sharing.error);
        assert_eq!(doc.working.notes, "Other device fixture");

        app.sharing.open = true;
        press(&ctx, &mut app, size, "Get these changes back");
        let doc = app.sharing.document.as_ref().unwrap();
        assert_eq!(doc.working.notes, "Second queued fixture", "{:?}", app.sharing.error);
        assert_eq!(doc.pending.len(), 1);
        drop(app); std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn large_conflict_comparison_is_lazy_cached_virtualized_and_leaves_resolution_reachable() {
        let (mut app, dir) = fixture();
        let doc = app.sharing.document.as_mut().unwrap();
        let mut snapshot = doc.working.clone();
        snapshot.notes = "A long Unicode fixture note: café 漢字. ".repeat(1_000);
        let payment = snapshot.payments[0].clone();
        snapshot.payments = (1..=200).map(|id| {
            let mut payment = payment.clone();
            payment.id = format!("fixture-{id}"); payment.payment.id = id; payment
        }).collect();
        snapshot.next_id = 201;
        doc.conflict = Some(Remote { id: snapshot.id.clone(), name: snapshot.name.clone(), revision: 2,
            payload: client::seal(&snapshot, &doc.recovery_key, 2).unwrap(), updated_at: "fixture-2".into(),
            role: "owner".into(), summary: "Fixture conflict".into() });
        let ctx = context(&app); let size = Vec2::new(900.0, 700.0);
        let mut output = settled(&ctx, &mut app, size, 1.0);
        let preview = app.sharing.preview.as_ref().unwrap();
        assert_eq!(preview.snapshot.as_ref().unwrap(), &snapshot);
        assert!(preview.complete.is_none(), "collapsed comparisons must not serialize full snapshots");
        let snapshot_address = preview.snapshot.as_ref().unwrap() as *const _;
        let pos = visible_text(&output, "Compare complete contents").unwrap().center();
        click(&ctx, &mut app, size, pos);
        output = settled(&ctx, &mut app, size, 1.0);
        let rows = app.sharing.preview.as_ref().unwrap().complete.as_ref().unwrap();
        assert!(rows.len() > 2_000, "the fixture must be large enough to require virtualization");
        assert!(rows.iter().all(|row| row.chars().count() <= 256));
        let rows_address = rows.as_ptr();
        for _ in 0..6 { output = frame(&ctx, &mut app, size, 1.0, vec![]); }
        let preview = app.sharing.preview.as_ref().unwrap();
        assert_eq!(preview.snapshot.as_ref().unwrap() as *const _, snapshot_address, "repaint must reuse decrypted conflict");
        assert_eq!(preview.complete.as_ref().unwrap().as_ptr(), rows_address, "repaint must reuse comparison rows");
        fn count_text(shape: &egui::Shape) -> usize {
            match shape { egui::Shape::Text(_) => 1, egui::Shape::Vec(shapes) => shapes.iter().map(count_text).sum(), _ => 0 }
        }
        let labels: usize = output.shapes.iter().map(|shape| count_text(&shape.shape)).sum();
        assert!(labels < 80, "only visible comparison rows may be painted, got {labels}");
        let output = scroll_to(&ctx, &mut app, size, "Use current shared version");
        assert!(visible_text(&output, "Close").is_some());
        assert!(app.sharing.document.as_ref().unwrap().conflict.is_some());

        // Replacing a remote revision must discard both the decrypted snapshot and old row cache.
        app.sharing.document.as_mut().unwrap().conflict.as_mut().unwrap().revision = 3;
        settled(&ctx, &mut app, size, 1.0);
        let preview = app.sharing.preview.as_ref().unwrap();
        assert_eq!(preview.revision, 3);
        assert!(preview.snapshot.is_err(), "changed authentication data must not reuse an old successful unlock");
        assert!(preview.complete.is_none());
        drop(app); std::fs::remove_dir_all(dir).unwrap();
    }
}
