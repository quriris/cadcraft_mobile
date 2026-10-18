//! Touch stand-ins for the keys and mouse button a tablet doesn't have.
//!
//! A small column of buttons on the right edge sends Enter and Esc (finishing and cancelling
//! commands, as on a keyboard), Ctrl+Z (undo), turns the next tap into a right-click (context
//! menus, "repeat last command") and shows the on-screen keyboard for the command line. The
//! Android Back button acts as Esc. All of it works by rewriting egui's raw input, so the app
//! sees ordinary key and pointer events.

use egui::{Event, Key, Modifiers, PointerButton, RawInput};

/// What a button press asks the host to do outside the input stream.
#[derive(Debug, PartialEq, Eq)]
pub enum Request {
    None,
    ShowKeyboard,
    OpenFile,
}

#[derive(Default)]
pub struct Controls {
    /// Keys to send with the next frame's input (key, modifiers).
    queued: Vec<(Key, Modifiers)>,
    /// The next tap is a right-click: armed by the button, spent by one press and its release.
    right_click: RightClick,
    /// The column is folded down to a single button.
    pub collapsed: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum RightClick {
    #[default]
    Off,
    Armed,
    /// The press went through as secondary; the matching release must too.
    Pressed,
}

impl Controls {
    pub fn right_click_armed(&self) -> bool {
        self.right_click != RightClick::Off
    }

    /// Rewrite this frame's raw input: queued button keys go in, Back becomes Esc, and an armed
    /// right-click turns the next primary press and release into secondary ones.
    pub fn rewrite(&mut self, raw: &mut RawInput) {
        for event in &mut raw.events {
            match event {
                Event::Key { key, .. } if *key == Key::BrowserBack => *key = Key::Escape,
                Event::PointerButton { button, pressed, .. } if *button == PointerButton::Primary => match (self.right_click, *pressed) {
                    (RightClick::Armed, true) => {
                        *button = PointerButton::Secondary;
                        self.right_click = RightClick::Pressed;
                    }
                    (RightClick::Pressed, false) => {
                        *button = PointerButton::Secondary;
                        self.right_click = RightClick::Off;
                    }
                    _ => {}
                },
                _ => {}
            }
        }
        for (key, modifiers) in self.queued.drain(..) {
            for pressed in [true, false] {
                raw.events.push(Event::Key { key, physical_key: Some(key), pressed, repeat: false, modifiers });
            }
        }
    }

    fn send(&mut self, key: Key, modifiers: Modifiers) {
        self.queued.push((key, modifiers));
    }

    /// Draw the button column on the right edge, over the drawing.
    pub fn ui(&mut self, ctx: &egui::Context, ru: bool) -> Request {
        let mut request = Request::None;
        let t = |en: &'static str, r: &'static str| if ru { r } else { en };
        let size = egui::vec2(72.0, 52.0);
        egui::Area::new(egui::Id::new("cadcraft_android_controls"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::RIGHT_CENTER, egui::vec2(-6.0, 0.0))
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
                    let button = |ui: &mut egui::Ui, label: &str, hint: &str, selected: bool| {
                        ui.add(egui::Button::new(egui::RichText::new(label).size(15.0)).selected(selected).min_size(size)).on_hover_text(hint).clicked()
                    };
                    if self.collapsed {
                        if button(ui, "◀", t("Show touch buttons", "Показать кнопки"), false) {
                            self.collapsed = false;
                        }
                        return;
                    }
                    if button(ui, t("Enter", "Ввод"), t("Enter: finish or repeat a command", "Enter: завершить или повторить команду"), false) {
                        self.send(Key::Enter, Modifiers::NONE);
                    }
                    if button(ui, "Esc", t("Esc: cancel the command", "Esc: отменить команду"), false) {
                        self.send(Key::Escape, Modifiers::NONE);
                    }
                    if button(ui, t("Undo", "Отмена"), t("Undo", "Отменить действие"), false) {
                        self.send(Key::Z, Modifiers { ctrl: true, command: true, ..Modifiers::NONE });
                    }
                    let armed = self.right_click_armed();
                    if button(ui, t("RMB", "ПКМ"), t("The next tap is a right-click", "Следующее касание — правый клик"), armed) {
                        self.right_click = if armed { RightClick::Off } else { RightClick::Armed };
                    }
                    if button(ui, t("Keys", "Клав."), t("On-screen keyboard", "Экранная клавиатура"), false) {
                        request = Request::ShowKeyboard;
                    }
                    if button(ui, t("Open", "Откр."), t("Open a drawing", "Открыть чертёж"), false) {
                        request = Request::OpenFile;
                    }
                    if button(ui, "▶", t("Hide these buttons", "Скрыть кнопки"), false) {
                        self.collapsed = true;
                    }
                });
            });
        request
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn click(pressed: bool) -> Event {
        Event::PointerButton { pos: egui::pos2(10.0, 10.0), button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }
    }

    fn buttons(raw: &RawInput) -> Vec<(PointerButton, bool)> {
        raw.events
            .iter()
            .filter_map(|e| match e {
                Event::PointerButton { button, pressed, .. } => Some((*button, *pressed)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn back_is_escape() {
        let mut c = Controls::default();
        let mut raw = RawInput {
            events: vec![Event::Key { key: Key::BrowserBack, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }],
            ..Default::default()
        };
        c.rewrite(&mut raw);
        assert!(matches!(raw.events.first(), Some(Event::Key { key: Key::Escape, .. })));
    }

    #[test]
    fn an_armed_right_click_turns_one_tap_secondary() {
        let mut c = Controls { right_click: RightClick::Armed, ..Default::default() };
        let mut raw = RawInput { events: vec![click(true)], ..Default::default() };
        c.rewrite(&mut raw);
        let mut up = RawInput { events: vec![click(false)], ..Default::default() };
        c.rewrite(&mut up);
        assert_eq!(buttons(&raw), [(PointerButton::Secondary, true)]);
        assert_eq!(buttons(&up), [(PointerButton::Secondary, false)]);
        assert!(!c.right_click_armed());
        // The tap after that is an ordinary one.
        let mut next = RawInput { events: vec![click(true), click(false)], ..Default::default() };
        c.rewrite(&mut next);
        assert_eq!(buttons(&next), [(PointerButton::Primary, true), (PointerButton::Primary, false)]);
    }

    #[test]
    fn queued_keys_are_pressed_and_released_once() {
        let mut c = Controls::default();
        c.send(Key::Enter, Modifiers::NONE);
        let mut raw = RawInput::default();
        c.rewrite(&mut raw);
        assert_eq!(raw.events.len(), 2);
        let mut again = RawInput::default();
        c.rewrite(&mut again);
        assert!(again.events.is_empty());
    }
}
