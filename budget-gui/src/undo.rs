//! Undo and Redo of applied edits.
//!
//! Each step keeps a snapshot of what the budget file stores (payments, balances and notes), never
//! the calendar built from it. Snapshots share every payment that did not change, so a step costs
//! about one payment however large the budget is. A new budget, or a change arriving from the
//! household, starts an empty history: an undo must never bring back what it replaced.

use budget_core::{BudgetDb, Cal, Payment};
use std::collections::{HashMap, HashSet, VecDeque};
use std::mem::{size_of, size_of_val};
use std::sync::Arc;

/// The most steps kept.
pub const MAX_STEPS: usize = 100;
/// The estimated memory a history may hold. Only budgets whose every step rewrites megabytes reach
/// it before `MAX_STEPS`; they then keep fewer steps.
pub const MAX_BYTES: usize = 256 << 20;
/// The longest name shown in the Edit menu.
const LABEL_CHARS: usize = 40;

/// What the budget file stores, as it stood at one moment.
#[derive(Clone)]
pub struct Snapshot {
    payments: Vec<Arc<Payment>>,
    adjustments: Arc<Payment>,
    notes: Arc<String>,
    /// Days in the calendar: each payment also holds a list of the days it falls on.
    days: usize,
}

impl Snapshot {
    /// Takes `db`, sharing what is unchanged since `previous`.
    fn capture(db: &BudgetDb, previous: Option<&Snapshot>) -> Snapshot {
        let known: HashMap<i32, &Arc<Payment>> = previous.iter().flat_map(|s| &s.payments).map(|p| (p.id, p)).collect();
        let shared = |p: &Payment| known.get(&p.id).copied().filter(|old| ***old == *p).cloned().unwrap_or_else(|| Arc::new(p.clone()));
        Snapshot {
            payments: db.payments.iter().map(shared).collect(),
            adjustments: previous.map(|s| &s.adjustments).filter(|old| ***old == db.adjustments).cloned().unwrap_or_else(|| Arc::new(db.adjustments.clone())),
            notes: previous.map(|s| &s.notes).filter(|old| ***old == db.notes).cloned().unwrap_or_else(|| Arc::new(db.notes.clone())),
            days: Cal::diff(&db.start_date, &db.end_date).max(0) as usize + 1,
        }
    }

    /// Puts this state back into `db`. The caller rebuilds the calendar. The next payment number
    /// stays where it is, so a number given up by an undone payment is not used for another.
    pub fn restore(&self, db: &mut BudgetDb) {
        db.payments = self.payments.iter().map(|p| (**p).clone()).collect();
        db.adjustments = (*self.adjustments).clone();
        db.notes = (*self.notes).clone();
    }
}

/// An estimate of the memory `p` holds; its list of calendar days is taken at its largest.
fn payment_bytes(p: &Payment, days: usize) -> usize {
    size_of::<Payment>() + p.description.len() + p.category.as_ref().map_or(0, String::len) + p.image_path.as_ref().map_or(0, String::len)
        + size_of_val(p.opt.as_slice()) + size_of_val(p.flag.as_slice()) + size_of_val(p.hilighting.as_slice())
        + size_of_val(p.note.as_slice()) + p.note.iter().map(|n| n.note.len()).sum::<usize>() + days * size_of::<usize>()
}

struct Step {
    /// What the edit was, such as "edit Rent".
    label: String,
    /// The budget on the far side of the edit.
    snapshot: Snapshot,
    /// Which state that is, to recognise the one last saved.
    id: u64,
}

/// A state brought back by Undo or Redo.
pub struct Restored {
    pub label: String,
    pub snapshot: Snapshot,
    /// This is the state last saved to the file.
    pub saved: bool,
}

/// The steps of the open budget.
#[derive(Default)]
pub struct History {
    /// The budget these steps belong to.
    generation: Option<u64>,
    current: Option<Snapshot>,
    id: u64,
    last_id: u64,
    saved: Option<u64>,
    undo: VecDeque<Step>,
    redo: Vec<Step>,
}

impl History {
    /// Starts over when the budget in the window is not the one these steps belong to. `dirty`: the
    /// budget now open already differs from its file, as a restored earlier version does.
    pub fn follow(&mut self, generation: u64, db: Option<&BudgetDb>, dirty: bool) {
        if self.generation == Some(generation) && (self.current.is_some() || db.is_none()) { return; }
        *self = History { generation: Some(generation), current: db.map(|db| Snapshot::capture(db, None)), saved: (!dirty).then_some(0), ..History::default() };
    }

    /// An edit named `label` has just been applied to `db`. New edits replace whatever Redo offered.
    pub fn record(&mut self, generation: u64, label: impl Into<String>, db: &BudgetDb) {
        let Some(before) = self.current.take().filter(|_| self.generation == Some(generation)) else {
            self.follow(generation, Some(db), true);
            return;
        };
        self.current = Some(Snapshot::capture(db, Some(&before)));
        self.undo.push_back(Step { label: short(&label.into()), snapshot: before, id: self.id });
        self.redo.clear();
        self.last_id += 1;
        self.id = self.last_id;
        self.trim(MAX_STEPS, MAX_BYTES);
    }

    /// Drops the oldest steps beyond the limits, always keeping the latest.
    fn trim(&mut self, steps: usize, bytes: usize) {
        while self.undo.len() > steps.max(1) { self.undo.pop_front(); }
        while self.undo.len() > 1 && self.memory() > bytes { self.undo.pop_front(); }
    }

    /// The estimated memory held, counting what snapshots share once.
    fn memory(&self) -> usize {
        let mut seen = HashSet::new();
        let mut total = 0;
        for s in self.undo.iter().map(|step| &step.snapshot).chain(self.redo.iter().map(|step| &step.snapshot)).chain(&self.current) {
            total += size_of_val(s.payments.as_slice());
            for p in s.payments.iter().chain([&s.adjustments]) {
                if seen.insert(Arc::as_ptr(p)) { total += payment_bytes(p, s.days); }
            }
            if seen.insert(Arc::as_ptr(&s.notes).cast()) { total += s.notes.len(); }
        }
        total
    }

    pub fn undo_label(&self) -> Option<&str> { self.undo.back().map(|step| step.label.as_str()) }
    pub fn redo_label(&self) -> Option<&str> { self.redo.last().map(|step| step.label.as_str()) }

    pub fn undo(&mut self) -> Option<Restored> {
        let step = self.undo.pop_back()?;
        let current = self.current.take()?;
        self.redo.push(Step { label: step.label.clone(), snapshot: current, id: self.id });
        Some(self.arrive(step))
    }

    pub fn redo(&mut self) -> Option<Restored> {
        let step = self.redo.pop()?;
        let current = self.current.take()?;
        self.undo.push_back(Step { label: step.label.clone(), snapshot: current, id: self.id });
        Some(self.arrive(step))
    }

    fn arrive(&mut self, step: Step) -> Restored {
        self.id = step.id;
        self.current = Some(step.snapshot.clone());
        Restored { label: step.label, snapshot: step.snapshot, saved: self.saved == Some(step.id) }
    }

    /// The budget as it stands was just written to its file.
    pub fn mark_saved(&mut self) { self.saved = Some(self.id); }
}

/// `label` cut to what the Edit menu can show.
fn short(label: &str) -> String {
    if label.chars().count() <= LABEL_CHARS { return label.to_string(); }
    format!("{}…", label.chars().take(LABEL_CHARS - 1).collect::<String>())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{calendar, document, editor::TransactionScope, lifecycle_tests::fixture, App, DeleteTarget};
    use crate::shared_client::{self as client, Connection, Remote, SharedDocument};
    use budget_core::sharing::{snapshot_from_db_with_operation, Snapshot as Shared};
    use budget_core::FLAG_NONE;
    use eframe::egui::{self, Event, Key, Modifiers, Vec2};
    use std::path::{Path, PathBuf};

    fn db(amount: f64) -> BudgetDb {
        let mut db = BudgetDb::new_with_today(&Cal::from_int(20261001), 12, false, 20261027);
        let mut p = Payment::new(20261027); p.description = "Rent".into(); p.amount = amount;
        db.add(p);
        let mut p = Payment::new(20261028); p.description = "Power".into(); p.amount = -80.0;
        db.add(p);
        db
    }
    fn set(db: &mut BudgetDb, amount: f64) { db.payments[0].amount = amount; }
    fn text(db: &BudgetDb) -> String { db.to_text(None).unwrap() }
    fn history(db: &BudgetDb) -> History { let mut h = History::default(); h.follow(1, Some(db), false); h }

    #[test]
    fn undo_and_redo_walk_the_steps_and_name_them() {
        let mut d = db(-1.0);
        let mut h = history(&d);
        assert!(h.undo_label().is_none() && h.redo_label().is_none());
        set(&mut d, -2.0); h.record(1, "edit Rent", &d);
        set(&mut d, -3.0); h.record(1, "change Rent", &d);
        assert_eq!(h.undo_label(), Some("change Rent"));
        h.undo().unwrap().snapshot.restore(&mut d);
        assert_eq!(d.payments[0].amount, -2.0);
        assert_eq!((h.undo_label(), h.redo_label()), (Some("edit Rent"), Some("change Rent")));
        h.undo().unwrap().snapshot.restore(&mut d);
        assert_eq!(d.payments[0].amount, -1.0);
        assert!(h.undo().is_none(), "nothing is left to undo");
        h.redo().unwrap().snapshot.restore(&mut d);
        h.redo().unwrap().snapshot.restore(&mut d);
        assert_eq!(d.payments[0].amount, -3.0);
        assert!(h.redo().is_none());
    }

    #[test]
    fn a_new_edit_replaces_what_redo_offered() {
        let mut d = db(-1.0);
        let mut h = history(&d);
        set(&mut d, -2.0); h.record(1, "first", &d);
        h.undo().unwrap().snapshot.restore(&mut d);
        set(&mut d, -9.0); h.record(1, "second", &d);
        assert!(h.redo_label().is_none());
        assert_eq!(h.undo_label(), Some("second"));
    }

    #[test]
    fn at_least_fifty_steps_are_kept_and_the_count_is_capped() {
        const { assert!(MAX_STEPS >= 50) };
        let mut d = db(-1.0);
        let mut h = history(&d);
        for i in 0..MAX_STEPS + 20 { set(&mut d, -(i as f64) - 2.0); h.record(1, format!("edit {i}"), &d); }
        assert_eq!(h.undo.len(), MAX_STEPS);
        let mut oldest = None;
        while let Some(step) = h.undo() { oldest = Some(step); }
        oldest.unwrap().snapshot.restore(&mut d);
        assert_eq!(d.payments[0].amount, -21.0, "the oldest steps went first");
    }

    #[test]
    fn snapshots_share_what_did_not_change_so_a_step_stays_small() {
        let mut d = db(-1.0);
        for i in 0..200 { let mut p = Payment::new(20261001); p.description = format!("Payment {i}"); d.add(p); }
        let mut h = history(&d);
        let first = h.current.clone().unwrap();
        set(&mut d, -2.0); h.record(1, "edit", &d);
        let second = h.current.as_ref().unwrap();
        assert!(!Arc::ptr_eq(&first.payments[0], &second.payments[0]));
        assert!(first.payments[1..].iter().zip(&second.payments[1..]).all(|(a, b)| Arc::ptr_eq(a, b)));
        assert!(Arc::ptr_eq(&first.adjustments, &second.adjustments) && Arc::ptr_eq(&first.notes, &second.notes));
        let one = h.memory();
        for i in 0..49 { set(&mut d, i as f64); h.record(1, "edit", &d); }
        assert!(h.memory() < one * 5, "fifty steps cost far less than fifty copies: {} and {one}", h.memory());
        d.payments.remove(0);
        let before = h.current.clone().unwrap();
        h.record(1, "delete", &d);
        assert!(Arc::ptr_eq(&before.payments[5], &h.current.as_ref().unwrap().payments[4]), "matched by payment, not position");
    }

    #[test]
    fn the_memory_limit_drops_the_oldest_steps_but_keeps_the_latest() {
        let mut d = db(-1.0);
        let mut h = history(&d);
        for i in 0..10 { set(&mut d, i as f64); h.record(1, format!("edit {i}"), &d); }
        let all = h.memory();
        h.trim(MAX_STEPS, all - 1);
        assert_eq!(h.undo.len(), 9);
        assert!(h.memory() < all);
        h.trim(MAX_STEPS, 0);
        assert_eq!(h.undo.len(), 1);
        assert_eq!(h.undo_label(), Some("edit 9"));
    }

    #[test]
    fn another_budget_starts_an_empty_history_but_the_same_budget_keeps_it() {
        let mut d = db(-1.0);
        let mut h = history(&d);
        set(&mut d, -2.0); h.record(1, "edit", &d);
        h.follow(1, Some(&d), true);
        assert!(h.undo_label().is_some());
        h.follow(2, Some(&d), false);
        assert!(h.undo_label().is_none());
    }

    #[test]
    fn a_long_name_is_cut_on_a_character() {
        assert_eq!(short("edit Rent"), "edit Rent");
        let cut = short(&"é".repeat(90));
        assert_eq!(cut.chars().count(), LABEL_CHARS);
        assert!(cut.ends_with('…'));
    }

    // ---- which state counts as saved ----

    fn arrives_saved(step: Option<Restored>) -> bool { step.unwrap().saved }

    #[test]
    fn undoing_back_to_the_saved_state_is_recognised_and_leaving_it_again_is_not() {
        let mut d = db(-1.0);
        let mut h = history(&d);
        set(&mut d, -2.0); h.record(1, "edit", &d);
        assert!(arrives_saved(h.undo()));
        assert!(!arrives_saved(h.redo()));
    }

    #[test]
    fn a_save_moves_the_saved_state_in_both_directions() {
        let mut d = db(-1.0);
        let mut h = history(&d);
        set(&mut d, -2.0); h.record(1, "edit", &d);
        h.mark_saved();
        assert!(!arrives_saved(h.undo()), "the opened state is no longer on disk");
        assert!(arrives_saved(h.redo()));
        h.undo();
        h.mark_saved();
        assert!(!arrives_saved(h.redo()));
        assert!(arrives_saved(h.undo()));
    }

    #[test]
    fn the_saved_state_dropped_with_redo_is_never_reached_again() {
        let mut d = db(-1.0);
        let mut h = history(&d);
        set(&mut d, -2.0); h.record(1, "first", &d);
        h.mark_saved();
        h.undo();
        set(&mut d, -3.0); h.record(1, "second", &d);
        assert!(!arrives_saved(h.undo()), "a new branch never counts as saved");
    }

    #[test]
    fn a_budget_opened_with_unsaved_changes_has_no_saved_state() {
        let mut d = db(-1.0);
        let mut h = History::default();
        h.follow(1, Some(&d), true);
        set(&mut d, -2.0); h.record(1, "edit", &d);
        assert!(!arrives_saved(h.undo()));
    }

    #[test]
    fn identical_budgets_from_different_edits_are_different_states() {
        // Returning to the saved figures by hand is an edit, and stays unsaved until Save.
        let mut d = db(-1.0);
        let mut h = history(&d);
        set(&mut d, -2.0); h.record(1, "one", &d);
        set(&mut d, -1.0); h.record(1, "two", &d);
        assert!(!arrives_saved(h.undo()));
        assert!(arrives_saved(h.undo()));
    }

    // ---- in the window ----

    fn app() -> (App, PathBuf) {
        let (mut app, dir) = fixture();
        app.saved_bytes = None;
        app.dirty = false;
        app.history_follow();
        (app, dir)
    }
    fn budget(app: &App) -> String { app.db.as_ref().unwrap().to_text(None).unwrap() }
    fn id(app: &App) -> i32 { app.db.as_ref().unwrap().payments[0].id }
    fn frame(app: &mut App, ctx: &egui::Context, events: Vec<Event>) -> egui::FullOutput {
        let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(1280.0, 840.0))), events, ..Default::default() };
        ctx.run(input, |ctx| app.render(ctx))
    }
    fn gui(app: &App) -> egui::Context {
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx);
        crate::theme::apply(&ctx, &app.t);
        ctx
    }
    fn key(key: Key, modifiers: Modifiers) -> Event { Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers } }
    fn ctrl() -> Modifiers { Modifiers::COMMAND | Modifiers::CTRL }

    /// Every kind of edit, applied the way the window applies it.
    #[test]
    fn each_kind_of_edit_undoes_and_redoes_exactly_and_is_named() {
        let edits: [(&str, fn(&mut App)); 7] = [
            ("add New bill", |app| { let mut p = Payment::new(20261020); p.description = "New bill".into(); p.amount = -5.0; app.apply_payment_edit(Box::new(p), None, None); }),
            ("edit Fixture payment", |app| { let i = id(app); let mut p = app.db.as_ref().unwrap().payments[0].clone(); p.amount = -44.0; app.apply_payment_edit(Box::new(p), Some(i), None); }),
            ("edit Fixture payment", |app| { let i = id(app); app.apply_transaction_edit(i, 20261027, -30.0, "", FLAG_NONE, TransactionScope::Occurrence); }),
            ("edit Fixture payment", |app| { let i = id(app); app.apply_transaction_edit(i, 20261027, -12.0, "A note", FLAG_NONE, TransactionScope::Occurrence); }),
            ("delete Fixture payment", |app| { let target = DeleteTarget { id: id(app), date: None }; app.apply_delete(target); }),
            ("move Fixture payment", |app| { let i = id(app); app.calendar_action(calendar::Action::MoveTransaction { id: i, from: 20261027, to: 20261105, amount: -12.0 }); }),
            ("balance", |app| app.apply_balance_edit(20261020, Some(900.0))),
        ];
        for (label, edit) in edits {
            let balance = format!("change balance on {}", crate::names::long_date(20261020));
            let label = if label == "balance" { balance.as_str() } else { label };
            let (mut app, dir) = app();
            let before = budget(&app);
            edit(&mut app);
            let after = budget(&app);
            assert_ne!(before, after, "{label}");
            assert!(app.dirty, "{label}");
            assert_eq!(app.undo.undo_label(), Some(label));
            app.step_history(true);
            assert_eq!(budget(&app), before, "undo {label}");
            assert!(!app.dirty, "{label} returned to the saved budget");
            app.step_history(false);
            assert_eq!(budget(&app), after, "redo {label}");
            assert!(app.dirty, "{label}");
            drop(app);
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn a_deleted_payment_comes_back_with_its_calendar_and_its_number_is_never_reused() {
        let (mut app, dir) = app();
        let gone = id(&app);
        app.apply_delete(DeleteTarget { id: gone, date: None });
        app.payment_list.selected = Some(gone);
        app.step_history(true);
        assert_eq!(id(&app), gone);
        let cal = Cal::from_int(20261027);
        assert_eq!(app.db.as_mut().unwrap().get_day(&cal).unwrap().transactions.len(), 1, "the calendar shows it again");
        let mut p = Payment::new(20261020); p.description = "Added".into();
        app.apply_payment_edit(Box::new(p), None, None);
        let added = app.db.as_ref().unwrap().payments[1].id;
        app.step_history(true);
        let mut p = Payment::new(20261021); p.description = "Added later".into();
        app.apply_payment_edit(Box::new(p), None, None);
        assert_ne!(app.db.as_ref().unwrap().payments[1].id, added);
        drop(app);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn undoing_an_added_payment_clears_its_selection() {
        let (mut app, dir) = app();
        let mut p = Payment::new(20261020); p.description = "Added".into();
        app.apply_payment_edit(Box::new(p), None, None);
        assert!(app.payment_list.selected.is_some());
        app.step_history(true);
        assert!(app.payment_list.selected.is_none());
        drop(app);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn undo_redraws_the_month_it_changes() {
        let (mut app, dir) = app();
        let revision = app.db.as_ref().unwrap().update_count;
        app.apply_balance_edit(20261020, Some(900.0));
        let edited = app.db.as_ref().unwrap().update_count;
        app.step_history(true);
        assert!(app.db.as_ref().unwrap().update_count > edited.max(revision), "the caches of the month must not survive");
        assert!(app.view.is_none());
        drop(app);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn saving_then_undoing_is_unsaved_and_redoing_is_saved_again() {
        let (mut app, dir) = app();
        app.apply_balance_edit(20261020, Some(900.0));
        assert!(app.save());
        assert!(!app.dirty);
        let on_disk = std::fs::read(app.file.as_ref().unwrap()).unwrap();
        app.step_history(true);
        assert!(app.dirty, "the file still has the balance");
        app.step_history(false);
        assert!(!app.dirty);
        assert_eq!(std::fs::read(app.file.as_ref().unwrap()).unwrap(), on_disk, "undo never writes the file");
        drop(app);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn undoing_back_to_saved_drops_the_stale_save_failure() {
        let (mut app, dir) = app();
        let file = app.file.clone().unwrap();
        app.apply_balance_edit(20261020, Some(900.0));
        std::fs::write(&file, b"another app").unwrap();
        app.saved_bytes = Some(b"opened".to_vec());
        assert!(!app.save());
        assert!(app.dialogs.save_error.is_some() && app.banner.is_some());
        app.step_history(true);
        assert!(!app.dirty);
        assert!(app.dialogs.save_error.is_none() && app.banner.is_none());
        drop(app);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_restored_earlier_version_stays_unsaved_however_far_it_is_undone() {
        let (mut app, dir) = fixture();
        app.saved_bytes = None;
        app.dirty = true;
        app.history_follow();
        app.apply_balance_edit(20261020, Some(900.0));
        app.step_history(true);
        assert!(app.dirty);
        drop(app);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn opening_or_closing_another_budget_clears_the_steps() {
        let (mut app, dir) = app();
        app.apply_balance_edit(20261020, Some(900.0));
        app.generation += 1;
        app.history_follow();
        assert!(app.undo.undo_label().is_none());
        app.apply_balance_edit(20261021, Some(800.0));
        assert!(app.undo.undo_label().is_some());
        app.close_budget();
        assert!(app.undo.undo_label().is_none());
        app.step_history(true);
        assert!(app.db.is_none());
        drop(app);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn nothing_is_undone_under_an_open_editor() {
        let (mut app, dir) = app();
        app.apply_balance_edit(20261020, Some(900.0));
        let edited = budget(&app);
        app.new_payment(20261020);
        let ctx = gui(&app);
        frame(&mut app, &ctx, vec![key(Key::Z, ctrl())]);
        assert_eq!(budget(&app), edited);
        drop(app);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_shortcuts_undo_and_redo_on_the_calendar() {
        let (mut app, dir) = app();
        let before = budget(&app);
        let ctx = gui(&app);
        frame(&mut app, &ctx, vec![]);
        app.apply_balance_edit(20261020, Some(900.0));
        let after = budget(&app);
        frame(&mut app, &ctx, vec![key(Key::Z, ctrl())]);
        assert_eq!(budget(&app), before, "Ctrl+Z");
        frame(&mut app, &ctx, vec![key(Key::Y, ctrl())]);
        assert_eq!(budget(&app), after, "Ctrl+Y");
        frame(&mut app, &ctx, vec![key(Key::Z, ctrl())]);
        frame(&mut app, &ctx, vec![key(Key::Z, ctrl() | Modifiers::SHIFT)]);
        assert_eq!(budget(&app), after, "Ctrl+Shift+Z is Redo, not a second Undo");
        drop(app);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_focused_text_box_keeps_ctrl_z() {
        let (mut app, dir) = app();
        let ctx = gui(&app);
        frame(&mut app, &ctx, vec![]);
        app.apply_balance_edit(20261020, Some(900.0));
        let edited = budget(&app);
        app.focus_filter = true;
        for _ in 0..3 { frame(&mut app, &ctx, vec![]); }
        frame(&mut app, &ctx, vec![Event::Text("rent".into())]);
        assert_eq!(app.filter_text, "rent");
        frame(&mut app, &ctx, vec![key(Key::Z, ctrl())]);
        frame(&mut app, &ctx, vec![]);
        assert_eq!(budget(&app), edited, "the budget is left alone");
        assert_eq!(app.filter_text, "", "the text box undid the typing");
        drop(app);
        let _ = std::fs::remove_dir_all(dir);
    }

    fn rendered(output: &egui::FullOutput) -> Vec<(String, egui::Pos2)> {
        let mut found = Vec::new();
        for clipped in &output.shapes {
            let mut stack = vec![&clipped.shape];
            while let Some(shape) = stack.pop() {
                match shape {
                    egui::Shape::Text(t) => found.push((t.galley.job.text.clone(), t.pos + t.galley.mesh_bounds.center().to_vec2())),
                    egui::Shape::Vec(v) => stack.extend(v),
                    _ => {}
                }
            }
        }
        found
    }
    fn click(app: &mut App, ctx: &egui::Context, pos: egui::Pos2) -> Vec<(String, egui::Pos2)> {
        frame(app, ctx, vec![Event::PointerMoved(pos)]);
        for pressed in [true, false] {
            frame(app, ctx, vec![Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE }]);
        }
        rendered(&frame(app, ctx, vec![]))
    }
    fn at(texts: &[(String, egui::Pos2)], label: &str) -> egui::Pos2 {
        texts.iter().find(|(t, _)| t == label).unwrap_or_else(|| panic!("{label} is missing from {:?}", texts.iter().map(|t| &t.0).collect::<Vec<_>>())).1
    }

    #[test]
    fn the_edit_menu_names_the_action_and_runs_it() {
        let (mut app, dir) = app();
        let before = budget(&app);
        let ctx = gui(&app);
        let i = id(&app);
        app.apply_transaction_edit(i, 20261027, -30.0, "", FLAG_NONE, TransactionScope::Occurrence);
        let texts = rendered(&frame(&mut app, &ctx, vec![]));
        let menu = click(&mut app, &ctx, at(&texts, "Edit"));
        assert!(menu.iter().any(|(t, _)| t == "Redo"), "with nothing to redo the item has no name");
        click(&mut app, &ctx, at(&menu, "Undo: edit Fixture payment"));
        assert_eq!(budget(&app), before);
        let menu = click(&mut app, &ctx, at(&texts, "Edit"));
        assert!(menu.iter().any(|(t, _)| t == "Redo: edit Fixture payment"));
        drop(app);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_undone_note_leaves_no_trace() {
        let mut d = db(-1.0);
        let mut h = history(&d);
        let before = text(&d);
        let id = d.payments[0].id;
        document::change_transaction(&mut d, id, 20261027, -1.0, "Paid by cheque", FLAG_NONE).unwrap();
        h.record(1, "edit Rent", &d);
        assert_ne!(text(&d), before);
        h.undo().unwrap().snapshot.restore(&mut d);
        d.rebuild_db(&Cal::from_int(20261001), 12, false);
        assert_eq!(text(&d), before);
    }

    // ---- a shared budget ----
    //
    // Saving a shared budget writes its protected cache, which is kept in the home folder. These
    // tests therefore run in a child process whose home folder is under `target/`.

    const ISOLATED: &str = "BUDGET_UNDO_TEST_HOME";

    #[test]
    fn shared_budget_steps_in_a_private_home_folder() {
        let home = std::path::absolute(Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/undo-tests")).unwrap().join(std::process::id().to_string());
        std::fs::create_dir_all(&home).unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["undo::tests::isolated_", "--ignored", "--test-threads=1"])
            .env("USERPROFILE", &home).env("HOME", &home).env(ISOLATED, &home)
            .output().unwrap();
        let log = String::from_utf8_lossy(&output.stdout);
        assert!(output.status.success(), "{log}");
        assert!(log.contains("3 passed"), "{log}");
        let _ = std::fs::remove_dir_all(home);
    }

    fn remote(s: &Shared, key: &str, revision: u64) -> Remote {
        Remote { id: s.id.clone(), name: s.name.clone(), revision, payload: client::seal(s, key, revision).unwrap(), updated_at: format!("fixture-{revision}"), role: "owner".into(), summary: "Fixture update".into() }
    }
    fn shared() -> (App, PathBuf) {
        assert!(std::env::var_os(ISOLATED).is_some(), "run through shared_budget_steps_in_a_private_home_folder");
        let (mut app, dir) = fixture();
        let s = snapshot_from_db_with_operation(app.db.as_ref().unwrap(), &uuid::Uuid::new_v4().to_string(), "Fixture household", "America/Halifax", None, &uuid::Uuid::new_v4().to_string()).unwrap();
        let key = client::generate_recovery_key();
        let doc = SharedDocument::open(Connection { server: "http://127.0.0.1:8787".into(), token: "fixture-session".into(), user: client::User { id: "fixture".into(), username: "fixture".into() } }, key.clone(), remote(&s, &key, 1)).unwrap();
        app.activate_shared(doc);
        assert!(app.sharing.document.is_some(), "{:?}", app.banner.as_ref().map(|b| &b.text));
        app.history_follow();
        (app, dir)
    }
    fn pending(app: &App) -> usize { app.sharing.document.as_ref().unwrap().pending.len() }
    /// The household as another device left it, for reopening over this one.
    fn from_elsewhere(mut doc: SharedDocument) -> SharedDocument {
        doc.working.name = "Remote household name".into(); doc.baseline = doc.working.clone(); doc.revision = 2;
        doc
    }

    #[test]
    #[ignore = "run by shared_budget_steps_in_a_private_home_folder"]
    fn isolated_a_shared_undo_is_a_new_edit_that_syncs_including_a_deleted_payment() {
        let (mut app, dir) = shared();
        let target = DeleteTarget { id: id(&app), date: None };
        app.apply_delete(target);
        assert!(app.db.as_ref().unwrap().payments.is_empty());
        assert_eq!(pending(&app), 1);
        app.step_history(true);
        let doc = app.sharing.document.as_ref().unwrap();
        assert_eq!(doc.pending.len(), 2, "undo queues its own change for the household");
        assert_eq!(doc.working.payments.len(), 1);
        assert_eq!(doc.working.payments[0].payment.description, "Fixture payment");
        assert!(!app.dirty, "it saved to this device like any edit");
        app.step_history(false);
        assert_eq!(pending(&app), 3);
        assert!(app.sharing.document.as_ref().unwrap().working.payments.is_empty());
        app.step_history(true);
        assert_eq!(app.sharing.document.as_ref().unwrap().working.payments.len(), 1, "and again after a redo");
        drop(app);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    #[ignore = "run by shared_budget_steps_in_a_private_home_folder"]
    fn isolated_undo_is_refused_while_a_conflict_is_open() {
        let (mut app, dir) = shared();
        let fresh = from_elsewhere(app.sharing.document.clone().unwrap());
        app.apply_balance_edit(20261020, Some(900.0));
        app.request_shared(fresh, &egui::Context::default());
        assert!(app.sharing.document.as_ref().unwrap().conflict.is_some());
        app.sharing.open = false;
        // An editor that was already open can still apply while the conflict stands.
        app.history_follow();
        app.db.as_mut().unwrap().notes = "Applied during the conflict".into();
        app.undo.record(app.generation, "edit Fixture payment", app.db.as_ref().unwrap());
        let edited = budget(&app);
        assert!(app.history_item(true).1.is_some(), "the menu item is disabled and says why");
        app.step_history(true);
        assert_eq!(budget(&app), edited);
        assert!(app.sharing.open, "the conflict is shown instead");
        drop(app);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    #[ignore = "run by shared_budget_steps_in_a_private_home_folder"]
    fn isolated_a_change_from_the_household_clears_the_steps_so_undo_cannot_overwrite_it() {
        let (mut app, dir) = shared();
        let fresh = from_elsewhere(app.sharing.document.clone().unwrap());
        app.apply_balance_edit(20261020, Some(900.0));
        assert!(app.undo.undo_label().is_some());
        let generation = app.generation;
        app.request_shared(fresh, &egui::Context::default());
        assert!(app.generation > generation);
        app.history_follow();
        assert!(app.undo.undo_label().is_none());
        drop(app);
        let _ = std::fs::remove_dir_all(dir);
    }
}
