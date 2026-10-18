//! Finger input for a drawing area built for a mouse.
//!
//! egui turns the first finger into a mouse: a press when it lands, moves while it slides, a
//! release and "pointer gone" when it lifts. Passed through as is, that breaks CAD work on a
//! tablet: the drawing area reads the cursor position on the frame a click ends, which the lift
//! has already cleared, so taps never place points; a second finger for pinch or pan leaves a
//! selection window started by the first; and there is no right button. This filter reshapes the
//! stream before the app sees it:
//!
//! - **Tap** = click, at the tap. The "pointer gone" of the lift waits a frame, so the click
//!   still has its position.
//! - **While a command asks for a point** (`aiming`), the finger is a cursor: it can slide to aim
//!   (snaps and the rubber band follow it) and the point is placed where it lifts.
//! - **With no command**, sliding past [`DRAG_START`] presses where the finger landed, so a drag
//!   makes a selection window or moves a grip, as with a mouse.
//! - **Two fingers** (pinch, pan) never press: a press held back for the first finger is dropped.
//! - **Long press** (held still for [`LONG_PRESS_S`], no command asking for a point) is a
//!   right-click.
//!
//! Mouse and stylus-with-buttons events (no touch in progress) pass through unchanged.

use egui::{Event, Modifiers, PointerButton, Pos2, RawInput, TouchPhase};

/// How far (points) a finger may slide and still be a tap; past it, a drag (no command running).
pub const DRAG_START: f32 = 12.0;
/// How long (seconds) a finger held still becomes a right-click.
pub const LONG_PRESS_S: f64 = 0.6;

#[derive(Default)]
pub struct TouchFilter {
    /// Fingers down now (touch ids).
    fingers: Vec<u64>,
    /// The first finger's press, held back: where it landed, when, and the modifiers.
    held: Option<Held>,
    /// The press reached the app (a drag); its release must too.
    pressed: bool,
    /// More than one finger since the first landed: nothing presses until all are up.
    gesture: bool,
    /// A long press fired a right-click: the lift sends nothing more.
    spent: bool,
    /// A "pointer gone" to deliver at the start of the next frame.
    gone_next: bool,
}

#[derive(Clone, Copy, Debug)]
struct Held {
    pos: Pos2,
    last: Pos2,
    at: f64,
    modifiers: Modifiers,
}

impl TouchFilter {
    /// True while a finger is down and its press is held back (the host repaints so a long press
    /// fires without new events).
    pub fn waiting(&self) -> bool {
        self.held.is_some()
    }

    /// Reshape this frame's events. `aiming`: a command is asking for a point.
    pub fn rewrite(&mut self, raw: &mut RawInput, aiming: bool) {
        let now = raw.time.unwrap_or(0.0);
        let events = std::mem::take(&mut raw.events);
        let mut out = Vec::with_capacity(events.len() + 3);
        if std::mem::take(&mut self.gone_next) {
            out.push(Event::PointerGone);
        }
        let mut released = false;
        for event in events {
            match event {
                Event::Touch { id, phase, .. } => {
                    let id = id.0;
                    match phase {
                        TouchPhase::Start => {
                            if !self.fingers.contains(&id) {
                                self.fingers.push(id);
                            }
                            if self.fingers.len() > 1 {
                                self.gesture = true;
                                self.held = None;
                            }
                        }
                        TouchPhase::End | TouchPhase::Cancel => self.fingers.retain(|f| *f != id),
                        TouchPhase::Move => {}
                    }
                    out.push(event);
                }
                Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers } if self.touching() => {
                    if !self.gesture {
                        self.held = Some(Held { pos, last: pos, at: now, modifiers });
                        self.spent = false;
                    }
                }
                Event::PointerMoved(pos) if self.touching() => {
                    if let Some(h) = &mut self.held {
                        h.last = pos;
                        if !aiming && !self.gesture && h.pos.distance(pos) > DRAG_START {
                            // A drag: press where the finger landed, then follow it.
                            out.push(Event::PointerMoved(h.pos));
                            out.push(Event::PointerButton { pos: h.pos, button: PointerButton::Primary, pressed: true, modifiers: h.modifiers });
                            self.pressed = true;
                            self.held = None;
                        }
                    }
                    out.push(event);
                }
                Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers } if self.lifting() => {
                    if let Some(h) = self.held.take() {
                        // A tap, or an aimed point: click where the finger lifted.
                        out.push(Event::PointerMoved(pos));
                        out.push(Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: h.modifiers });
                        out.push(Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers });
                        released = true;
                    } else if std::mem::take(&mut self.pressed) {
                        out.push(event);
                        released = true;
                    }
                    // Otherwise the press never reached the app (a gesture, or a spent long press).
                    self.spent = false;
                }
                Event::PointerGone if released => self.gone_next = true,
                other => out.push(other),
            }
        }
        if self.fingers.is_empty() {
            self.gesture = false;
        }
        // A finger held still: right-click where it is (not while aiming: a finger resting on the
        // point it is about to place must not end the command; ⏎ does that).
        if let Some(h) = self.held
            && !aiming
            && !self.gesture
            && now - h.at >= LONG_PRESS_S
            && h.pos.distance(h.last) <= DRAG_START
        {
            out.push(Event::PointerButton { pos: h.last, button: PointerButton::Secondary, pressed: true, modifiers: h.modifiers });
            out.push(Event::PointerButton { pos: h.last, button: PointerButton::Secondary, pressed: false, modifiers: h.modifiers });
            self.held = None;
            self.spent = true;
        }
        raw.events = out;
    }

    /// A finger is down (or one was, until its lift is handled).
    fn touching(&self) -> bool {
        !self.fingers.is_empty() || self.held.is_some()
    }

    /// The release that belongs to a touch: one was held, pressed, gestured or spent.
    fn lifting(&self) -> bool {
        self.held.is_some() || self.pressed || self.gesture || self.spent || !self.fingers.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(id: u64, phase: TouchPhase, x: f32) -> Event {
        Event::Touch { device_id: egui::TouchDeviceId(0), id: egui::TouchId(id), phase, pos: egui::pos2(x, 0.0), force: None }
    }
    fn button(x: f32, pressed: bool) -> Event {
        Event::PointerButton { pos: egui::pos2(x, 0.0), button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }
    }
    fn moved(x: f32) -> Event {
        Event::PointerMoved(egui::pos2(x, 0.0))
    }
    fn frame(f: &mut TouchFilter, t: f64, events: Vec<Event>, aiming: bool) -> Vec<Event> {
        let mut raw = RawInput { time: Some(t), events, ..Default::default() };
        f.rewrite(&mut raw, aiming);
        raw.events.into_iter().filter(|e| !matches!(e, Event::Touch { .. })).collect()
    }
    fn presses(events: &[Event]) -> Vec<(PointerButton, bool, f32)> {
        events
            .iter()
            .filter_map(|e| match e {
                Event::PointerButton { button, pressed, pos, .. } => Some((*button, *pressed, pos.x)),
                _ => None,
            })
            .collect()
    }

    /// A finger landing and lifting as egui-winit reports it (touch, move, press / touch, release, gone).
    fn down(x: f32, id: u64) -> Vec<Event> {
        vec![touch(id, TouchPhase::Start, x), moved(x), button(x, true)]
    }
    fn up(x: f32, id: u64) -> Vec<Event> {
        vec![touch(id, TouchPhase::End, x), button(x, false), Event::PointerGone]
    }

    #[test]
    fn a_tap_clicks_and_keeps_its_position_for_a_frame() {
        let mut f = TouchFilter::default();
        assert!(presses(&frame(&mut f, 0.0, down(10.0, 1), false)).is_empty(), "the press is held back");
        let lift = frame(&mut f, 0.1, up(10.0, 1), false);
        assert_eq!(presses(&lift), [(PointerButton::Primary, true, 10.0), (PointerButton::Primary, false, 10.0)]);
        assert!(!lift.iter().any(|e| matches!(e, Event::PointerGone)), "gone waits a frame");
        let next = frame(&mut f, 0.2, vec![], false);
        assert!(matches!(next.first(), Some(Event::PointerGone)));
    }

    #[test]
    fn a_tap_within_one_frame_still_clicks() {
        let mut f = TouchFilter::default();
        let mut events = down(5.0, 1);
        events.extend(up(5.0, 1));
        assert_eq!(presses(&frame(&mut f, 0.0, events, false)), [(PointerButton::Primary, true, 5.0), (PointerButton::Primary, false, 5.0)]);
    }

    #[test]
    fn while_aiming_the_point_goes_where_the_finger_lifts() {
        let mut f = TouchFilter::default();
        frame(&mut f, 0.0, down(10.0, 1), true);
        let slide = frame(&mut f, 0.1, vec![touch(1, TouchPhase::Move, 80.0), moved(80.0)], true);
        assert!(presses(&slide).is_empty(), "sliding only aims");
        let lift = frame(&mut f, 0.2, up(80.0, 1), true);
        assert_eq!(presses(&lift), [(PointerButton::Primary, true, 80.0), (PointerButton::Primary, false, 80.0)]);
    }

    #[test]
    fn with_no_command_a_slide_is_a_drag_from_where_it_landed() {
        let mut f = TouchFilter::default();
        frame(&mut f, 0.0, down(10.0, 1), false);
        let slide = frame(&mut f, 0.1, vec![touch(1, TouchPhase::Move, 80.0), moved(80.0)], false);
        assert_eq!(presses(&slide), [(PointerButton::Primary, true, 10.0)]);
        let lift = frame(&mut f, 0.2, up(80.0, 1), false);
        assert_eq!(presses(&lift), [(PointerButton::Primary, false, 80.0)]);
    }

    #[test]
    fn two_fingers_never_press() {
        let mut f = TouchFilter::default();
        frame(&mut f, 0.0, down(10.0, 1), false);
        let second = frame(&mut f, 0.05, vec![touch(2, TouchPhase::Start, 50.0)], false);
        let pinch = frame(&mut f, 0.1, vec![touch(1, TouchPhase::Move, 0.0), moved(0.0), touch(2, TouchPhase::Move, 90.0)], false);
        let lift2 = frame(&mut f, 0.2, vec![touch(2, TouchPhase::End, 90.0)], false);
        let lift1 = frame(&mut f, 0.3, up(0.0, 1), false);
        for events in [&second, &pinch, &lift2, &lift1] {
            assert!(presses(events).is_empty(), "{events:?}");
        }
        // The next tap is an ordinary one again.
        frame(&mut f, 1.0, down(30.0, 3), false);
        assert_eq!(presses(&frame(&mut f, 1.1, up(30.0, 3), false)).len(), 2);
    }

    #[test]
    fn a_long_press_is_a_right_click_and_its_lift_is_silent() {
        let mut f = TouchFilter::default();
        frame(&mut f, 0.0, down(10.0, 1), false);
        assert!(f.waiting());
        let held = frame(&mut f, 0.7, vec![], false);
        assert_eq!(presses(&held), [(PointerButton::Secondary, true, 10.0), (PointerButton::Secondary, false, 10.0)]);
        assert!(presses(&frame(&mut f, 0.8, up(10.0, 1), false)).is_empty());
    }

    #[test]
    fn resting_while_aiming_is_not_a_right_click() {
        let mut f = TouchFilter::default();
        frame(&mut f, 0.0, down(10.0, 1), true);
        assert!(presses(&frame(&mut f, 2.0, vec![], true)).is_empty());
        assert_eq!(presses(&frame(&mut f, 2.1, up(10.0, 1), true)), [(PointerButton::Primary, true, 10.0), (PointerButton::Primary, false, 10.0)]);
    }

    #[test]
    fn mouse_clicks_pass_through() {
        let mut f = TouchFilter::default();
        let events = frame(&mut f, 0.0, vec![moved(4.0), button(4.0, true), button(4.0, false)], false);
        assert_eq!(presses(&events), [(PointerButton::Primary, true, 4.0), (PointerButton::Primary, false, 4.0)]);
    }
}
