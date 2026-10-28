//! A dated closing-balance draft. The parent applies returned values to the document.
use eframe::egui::{self, RichText, TextEdit};
use crate::theme::{self, Tokens};

#[derive(Debug, PartialEq)]
pub enum BalanceAction { None, Cancel, Apply(f64), Remove }

pub struct BalanceEditor {
    pub date: i32,
    amount: String,
    // The seeded 2-decimal text and the exact value it stands for: applying unchanged text keeps
    // the exact value rather than storing the rounded display text.
    seed_text: String,
    seed_value: f64,
    adjusted: bool,
    error: Option<String>,
    focus_amount: bool,
}

impl BalanceEditor {
    pub fn new(date: i32, current: f64, adjusted: bool) -> Self {
        let mut seed_text = format!("{current:.2}");
        if seed_text == "-0.00" { seed_text = "0.00".into(); }
        Self { date, amount: seed_text.clone(), seed_text, seed_value: current, adjusted, error: None, focus_amount: true }
    }

    pub fn set_error(&mut self, error: String) { self.error = Some(error); }

    fn apply(&mut self) -> BalanceAction {
        if self.amount == self.seed_text { return BalanceAction::Apply(self.seed_value); }
        let parsed = if self.amount.trim().is_empty() {
            Err("Enter a closing balance.".to_string())
        } else {
            crate::editor::parse_amount(&self.amount, "Closing balance")
        };
        match parsed {
            Ok(amount) => BalanceAction::Apply(amount),
            Err(error) => { self.error = Some(error); BalanceAction::None }
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, t: &Tokens) -> BalanceAction {
        if crate::editor::dialog_escape(ctx) {
            return BalanceAction::Cancel;
        }
        let mut action = BalanceAction::None;
        let mut enter = false;
        egui::Modal::new(egui::Id::new("balance_editor")).frame(t.card().inner_margin(24)).show(ctx, |ui| {
            ui.set_width(480.0_f32.min((ctx.screen_rect().width() - 96.0).max(240.0)));
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 10.0);
            ui.heading("Edit balance");
            ui.horizontal_wrapped(|ui| {
                theme::badge(ui, "Balance adjustment", t.accent, t);
                ui.label(RichText::new(display_date(self.date)).color(t.text_muted));
            });
            ui.label(RichText::new("Sets the closing balance for this date and updates following balances until the next balance adjustment.").color(t.text_muted));
            ui.add_space(2.0);
            egui::Frame::new().fill(t.bg).corner_radius(8).inner_margin(16).show(ui, |ui| {
                ui.set_width(ui.available_width());
                let label = ui.label(RichText::new("Closing balance").strong().color(t.text));
                let mut output = TextEdit::singleline(&mut self.amount)
                    .id_salt("balance_amount")
                    .desired_width(f32::INFINITY)
                    .font(egui::TextStyle::Heading)
                    .horizontal_align(egui::Align::RIGHT)
                    .hint_text("0.00")
                    .show(ui);
                output.response = output.response.labelled_by(label.id);
                enter = crate::editor::submitted(ui, &output.response);
                if self.focus_amount {
                    output.response.request_focus();
                    output.state.cursor.set_char_range(Some(egui::text::CCursorRange::two(
                        egui::text::CCursor::new(0), egui::text::CCursor::new(self.amount.chars().count()),
                    )));
                    output.state.store(ctx, output.response.id);
                    self.focus_amount = false;
                }
                ui.label(RichText::new("Use a minus sign for a negative balance.").color(t.text_muted));
            });
            if let Some(error) = &self.error {
                egui::Frame::new().fill(theme::tint(t.surface, t.negative, 0.08)).corner_radius(6)
                    .inner_margin(10).show(ui, |ui| {
                        egui::ScrollArea::vertical().id_salt("balance_error")
                            .max_height(90.0).show(ui, |ui| {
                                ui.label(RichText::new(error).color(t.negative));
                            });
                    });
            }
            ui.add_space(4.0);
            ui.separator();
            ui.horizontal(|ui| {
                if theme::secondary_button(ui, "Cancel", t).clicked() { action = BalanceAction::Cancel; }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if theme::primary_button(ui, "Apply balance", t).clicked() { action = self.apply(); }
                });
            });
            if self.adjusted {
                ui.add_space(2.0);
                let remove = ui.add(egui::Button::new(RichText::new("Remove adjustment").color(t.negative))
                    .fill(egui::Color32::TRANSPARENT).stroke(egui::Stroke::NONE))
                    .on_hover_text("Use the balance calculated from earlier dates again.");
                crate::theme::focus_ring(ui, &remove, t);
                if remove.clicked() {
                    action = BalanceAction::Remove;
                }
            }
        });
        if enter && matches!(action, BalanceAction::None) { action = self.apply(); }
        action
    }
}

fn display_date(date: i32) -> String { crate::names::long_date(date) }

#[cfg(test)]
mod tests {
    use super::*;
    use budget_core::config::{Colors, Theme};

    #[test]
    fn applying_the_seed_preserves_the_exact_signed_current_balance() {
        for current in [-1234.56789012345, 9876.54321098765, -0.0, 0.0, f64::MIN_POSITIVE, -f64::MIN_POSITIVE] {
            let mut editor = BalanceEditor::new(20261027, current, false);
            let BalanceAction::Apply(amount) = editor.apply() else { panic!("A finite seed must apply"); };
            assert_eq!(amount.to_bits(), current.to_bits(), "The edit seed must not round {current}");
        }
    }

    #[test]
    fn the_target_date_is_retained_and_displayed_as_a_readable_calendar_date() {
        let editor = BalanceEditor::new(20240229, 1.0, true);
        assert_eq!(editor.date, 20240229);
        assert_eq!(display_date(editor.date), "February 29, 2024");
        assert_eq!(display_date(20261231), "December 31, 2026");
        assert_eq!(display_date(20270101), "January 1, 2027");
    }

    #[test]
    fn applying_accepts_finite_signed_balances_and_surrounding_whitespace() {
        for (text, expected) in [(" -42.25 ", -42.25_f64), ("+51.5", 51.5), ("0", 0.0), ("-0", -0.0), ("1e3", 1000.0)] {
            let mut editor = BalanceEditor::new(20261027, 10.0, false);
            editor.amount = text.into();
            let BalanceAction::Apply(amount) = editor.apply() else { panic!("A signed finite value must apply: {text}"); };
            assert_eq!(amount.to_bits(), expected.to_bits());
        }
    }

    #[test]
    fn invalid_or_nonfinite_balances_keep_the_draft_and_return_no_action() {
        for text in ["", "   ", "balance", "--2", "12.3.4", "NaN", "inf", "-inf", "1e999"] {
            let mut editor = BalanceEditor::new(20261027, 10.0, false);
            editor.amount = text.into();
            assert_eq!(editor.apply(), BalanceAction::None, "Invalid input must not apply: {text}");
            assert!(editor.error.as_ref().is_some_and(|error| !error.is_empty()), "Invalid input needs an error: {text}");
            assert_eq!(editor.amount, text);
        }
    }

    #[test]
    fn a_document_error_keeps_the_typed_balance_for_correction_or_retry() {
        let mut editor = BalanceEditor::new(20261027, 10.0, true);
        editor.amount = "-456.789".into();
        editor.set_error("The target date is outside this budget.".into());
        assert_eq!(editor.amount, "-456.789");
        assert_eq!(editor.date, 20261027);
        assert_eq!(editor.error.as_deref(), Some("The target date is outside this budget."));
    }

    #[test]
    fn escape_cancels_a_typed_draft_without_returning_an_applied_value() {
        let ctx = egui::Context::default();
        let tokens = Tokens::new(&Colors::theme(Theme::Light), false);
        let mut editor = BalanceEditor::new(20261027, 10.0, true);
        editor.amount = "-456.789".into();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 600.0))),
            events: vec![egui::Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }],
            ..Default::default()
        };
        let mut action = BalanceAction::None;
        let _ = ctx.run(input, |ctx| { action = editor.show(ctx, &tokens); });
        assert_eq!(action, BalanceAction::Cancel);
        assert_eq!(editor.amount, "-456.789");
        assert_eq!(editor.date, 20261027);
    }

    #[test]
    fn the_seed_is_two_decimal_text_and_edits_accept_grouping() {
        let editor = BalanceEditor::new(20261027, 0.1 + 0.2, false);
        assert_eq!(editor.amount, "0.30");
        assert_eq!(BalanceEditor::new(20261027, -0.0, false).amount, "0.00");
        let mut editor = BalanceEditor::new(20261027, 10.0, false);
        editor.amount = "1,234.56".into();
        assert_eq!(editor.apply(), BalanceAction::Apply(1234.56));
        editor.amount = "1e308".into();
        assert_eq!(editor.apply(), BalanceAction::None);
        assert!(editor.error.as_deref().unwrap().contains("too large"));
    }

    #[test]
    fn enter_applies_the_balance() {
        let ctx = egui::Context::default();
        let tokens = Tokens::new(&Colors::theme(Theme::Light), false);
        let mut editor = BalanceEditor::new(20261027, 10.0, false);
        theme::install_fonts(&ctx); theme::apply(&ctx, &tokens);
        let size = egui::vec2(900.0, 600.0);
        let mut run = |events: Vec<egui::Event>| {
            let mut action = BalanceAction::None;
            let _ = ctx.run(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)), events, ..Default::default() },
                |ctx| { action = editor.show(ctx, &tokens); });
            action
        };
        for _ in 0..3 { run(Vec::new()); }
        let action = run(vec![egui::Event::Key { key: egui::Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }]);
        assert_eq!(action, BalanceAction::Apply(10.0));
    }
}
