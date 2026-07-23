use std::ops::Range;

use eframe::egui::{
    self, Align2, Color32, Event, FontId, ImeEvent, Key, Sense, Stroke, StrokeKind, WidgetInfo,
    vec2,
};
use keeless_secure_types::{Error, SecureBytes};
use subtle::ConstantTimeEq;
use zeroize::Zeroize;

const MAX_PASSWORD_BYTES: usize = 4096;

pub struct SecureTextBuffer {
    bytes: SecureBytes,
    len: usize,
}

impl SecureTextBuffer {
    pub fn new() -> Result<Self, Error> {
        Ok(Self {
            bytes: SecureBytes::from_slice(&[0; MAX_PASSWORD_BYTES])?,
            len: 0,
        })
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[cfg(test)]
    pub fn from_text(value: &str) -> Result<Self, SecureTextEditError> {
        let mut buffer = Self::new()?;
        buffer.insert(0, value)?;
        Ok(buffer)
    }

    pub fn char_len(&self) -> Result<usize, Error> {
        self.with_str(|value| value.chars().count())
    }

    pub fn with_str<R>(&self, use_value: impl FnOnce(&str) -> R) -> Result<R, Error> {
        self.bytes.unlock_slice(|bytes| {
            let value = std::str::from_utf8(&bytes[..self.len])
                .expect("secure text buffer UTF-8 invariant");
            use_value(value)
        })
    }

    pub fn equals(&self, other: &Self) -> Result<bool, Error> {
        self.bytes.unlock_slice(|left| {
            other.bytes.unlock_slice(|right| {
                bool::from(
                    left.ct_eq(right) & self.len.to_ne_bytes().ct_eq(&other.len.to_ne_bytes()),
                )
            })
        })?
    }

    fn insert(&mut self, char_index: usize, value: &str) -> Result<usize, SecureTextEditError> {
        let byte_index = self.byte_index(char_index)?;
        let available = MAX_PASSWORD_BYTES - self.len;
        if value.len() > available {
            return Err(SecureTextEditError::Capacity);
        }
        if value.is_empty() {
            return Ok(0);
        }

        self.bytes.unlock_slice_mut(|bytes| {
            bytes.copy_within(byte_index..self.len, byte_index + value.len());
            bytes[byte_index..byte_index + value.len()].copy_from_slice(value.as_bytes());
        })?;
        self.len += value.len();
        Ok(value.chars().count())
    }

    fn delete(&mut self, chars: Range<usize>) -> Result<(), SecureTextEditError> {
        if chars.start >= chars.end {
            return Ok(());
        }
        let start = self.byte_index(chars.start)?;
        let end = self.byte_index(chars.end)?;
        let removed = end - start;
        self.bytes.unlock_slice_mut(|bytes| {
            bytes.copy_within(end..self.len, start);
            bytes[self.len - removed..self.len].zeroize();
        })?;
        self.len -= removed;
        Ok(())
    }

    fn byte_index(&self, char_index: usize) -> Result<usize, Error> {
        self.with_str(|value| {
            if char_index == value.chars().count() {
                value.len()
            } else {
                value
                    .char_indices()
                    .nth(char_index)
                    .map_or(value.len(), |(index, _)| index)
            }
        })
    }
}

#[derive(Default)]
pub struct SecureTextEditState {
    cursor: usize,
    offset: f32,
}

pub struct SecureTextEditOutput {
    pub response: egui::Response,
    pub submitted: bool,
    pub error: Option<SecureTextEditError>,
}

#[derive(Clone, Copy, Debug, thiserror::Error)]
pub enum SecureTextEditError {
    #[error("{0}")]
    Memory(#[from] Error),
    #[error("password exceeds {MAX_PASSWORD_BYTES} UTF-8 bytes")]
    Capacity,
}

pub fn secure_text_edit(
    ui: &mut egui::Ui,
    id_salt: impl std::hash::Hash,
    text: &mut SecureTextBuffer,
    state: &mut SecureTextEditState,
    hint: &str,
) -> SecureTextEditOutput {
    let id = ui.make_persistent_id(id_salt);
    let desired_size = vec2(ui.available_width(), 34.0);
    let (_, rect) = ui.allocate_space(desired_size);
    let mut response = ui.interact(rect, id, Sense::click());
    if response.clicked() {
        response.request_focus();
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
        ui.output_mut(|output| output.mutable_text_under_cursor = true);
    }

    let mut submitted = false;
    let mut error = None;
    let mut changed = false;
    let initial_char_len = match text.char_len() {
        Ok(value) => value,
        Err(value) => {
            error = Some(value.into());
            0
        }
    };
    state.cursor = state.cursor.min(initial_char_len);

    if response.has_focus() && error.is_none() {
        let mut events = ui.input_mut(|input| {
            let events = std::mem::take(&mut input.events);
            let (events, remaining) = events.into_iter().partition(is_text_event);
            input.events = remaining;
            events
        });
        for event in &mut events {
            let char_len = match text.char_len() {
                Ok(value) => value,
                Err(value) => {
                    error = Some(value.into());
                    break;
                }
            };
            let result = match event {
                Event::Copy | Event::Cut => Ok(()),
                Event::Text(value) | Event::Paste(value) => {
                    if value != "\n" && value != "\r" {
                        match text.insert(state.cursor, value) {
                            Ok(inserted) => {
                                state.cursor += inserted;
                                changed |= inserted > 0;
                                Ok(())
                            }
                            Err(error) => Err(error),
                        }
                    } else {
                        Ok(())
                    }
                }
                Event::Ime(ImeEvent::Commit(value)) => match text.insert(state.cursor, value) {
                    Ok(inserted) => {
                        state.cursor += inserted;
                        changed |= inserted > 0;
                        Ok(())
                    }
                    Err(error) => Err(error),
                },
                Event::Ime(ImeEvent::Preedit(_))
                | Event::Ime(ImeEvent::Enabled | ImeEvent::Disabled) => Ok(()),
                Event::Key {
                    key: Key::Backspace,
                    pressed: true,
                    ..
                } if state.cursor > 0 => {
                    let previous = state.cursor - 1;
                    let result = text.delete(previous..state.cursor);
                    if result.is_ok() {
                        state.cursor = previous;
                        changed = true;
                    }
                    result
                }
                Event::Key {
                    key: Key::Delete,
                    pressed: true,
                    ..
                } if state.cursor < char_len => {
                    let result = text.delete(state.cursor..state.cursor + 1);
                    changed |= result.is_ok();
                    result
                }
                Event::Key {
                    key: Key::ArrowLeft,
                    pressed: true,
                    ..
                } => {
                    state.cursor = state.cursor.saturating_sub(1);
                    Ok(())
                }
                Event::Key {
                    key: Key::ArrowRight,
                    pressed: true,
                    ..
                } => {
                    state.cursor = (state.cursor + 1).min(char_len);
                    Ok(())
                }
                Event::Key {
                    key: Key::Home,
                    pressed: true,
                    ..
                } => {
                    state.cursor = 0;
                    Ok(())
                }
                Event::Key {
                    key: Key::End,
                    pressed: true,
                    ..
                } => {
                    state.cursor = char_len;
                    Ok(())
                }
                Event::Key {
                    key: Key::Enter,
                    pressed: true,
                    ..
                } => {
                    submitted = true;
                    Ok(())
                }
                _ => Ok(()),
            };
            zeroize_event(event);
            if let Err(value) = result {
                error = Some(value);
                break;
            }
        }
        for event in &mut events {
            zeroize_event(event);
        }
    }

    if changed {
        response.mark_changed();
    }
    paint(ui, rect, &response, text, state, hint, error.is_some());
    response.widget_info(|| WidgetInfo::text_edit(ui.is_enabled(), "", ""));
    SecureTextEditOutput {
        response,
        submitted,
        error,
    }
}

fn is_text_event(event: &Event) -> bool {
    matches!(
        event,
        Event::Copy
            | Event::Cut
            | Event::Paste(_)
            | Event::Text(_)
            | Event::Key { .. }
            | Event::Ime(_)
    )
}

fn zeroize_event(event: &mut Event) {
    match event {
        Event::Paste(value) | Event::Text(value) => value.zeroize(),
        Event::Ime(ImeEvent::Preedit(value) | ImeEvent::Commit(value)) => value.zeroize(),
        _ => {}
    }
}

fn paint(
    ui: &egui::Ui,
    rect: egui::Rect,
    response: &egui::Response,
    text: &SecureTextBuffer,
    state: &mut SecureTextEditState,
    hint: &str,
    failed: bool,
) {
    let visuals = ui.style().interact(response);
    let stroke = if failed {
        Stroke::new(1.0, ui.visuals().error_fg_color)
    } else {
        visuals.bg_stroke
    };
    ui.painter()
        .rect_filled(rect, visuals.corner_radius, ui.visuals().extreme_bg_color);
    ui.painter()
        .rect_stroke(rect, visuals.corner_radius, stroke, StrokeKind::Inside);

    let inner = rect.shrink2(vec2(8.0, 4.0));
    let font = FontId::proportional(16.0);
    let color = if ui.is_enabled() {
        ui.visuals().text_color()
    } else {
        Color32::GRAY
    };
    let count = text.char_len().unwrap_or(0);
    if count == 0 {
        ui.painter().text(
            inner.left_center(),
            Align2::LEFT_CENTER,
            hint,
            font,
            ui.visuals().weak_text_color(),
        );
        state.offset = 0.0;
        return;
    }

    let masked: String =
        std::iter::repeat_n(egui::epaint::text::PASSWORD_REPLACEMENT_CHAR, count).collect();
    let galley = ui.painter().layout_no_wrap(masked, font, color);
    let cursor_x = galley.size().x * state.cursor as f32 / count as f32;
    if cursor_x - state.offset > inner.width() {
        state.offset = cursor_x - inner.width();
    } else if cursor_x < state.offset {
        state.offset = cursor_x;
    }
    state.offset = state.offset.max(0.0);
    let origin = inner.left_center() - vec2(state.offset, galley.size().y / 2.0);
    ui.painter()
        .with_clip_rect(inner)
        .galley(origin, galley, color);

    if response.has_focus() && ui.input(|input| input.focused) {
        let cursor = inner.left() + cursor_x - state.offset;
        ui.painter().line_segment(
            [
                egui::pos2(cursor, inner.top()),
                egui::pos2(cursor, inner.bottom()),
            ],
            Stroke::new(1.0, color),
        );
        ui.ctx().output_mut(|output| {
            output.ime = Some(egui::output::IMEOutput {
                rect,
                cursor_rect: egui::Rect::from_min_max(
                    egui::pos2(cursor, inner.top()),
                    egui::pos2(cursor + 1.0, inner.bottom()),
                ),
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_ascii_and_unicode_without_reallocation() {
        let mut value = SecureTextBuffer::new().unwrap();
        assert_eq!(value.insert(0, "pass").unwrap(), 4);
        assert_eq!(value.insert(2, "한").unwrap(), 1);
        assert_eq!(value.with_str(str::to_owned).unwrap(), "pa한ss");
        value.delete(1..3).unwrap();
        assert_eq!(value.with_str(str::to_owned).unwrap(), "pss");
    }

    #[test]
    fn compares_without_exporting_plaintext() {
        let mut left = SecureTextBuffer::new().unwrap();
        let mut right = SecureTextBuffer::new().unwrap();
        left.insert(0, "secret").unwrap();
        right.insert(0, "secret").unwrap();
        assert!(left.equals(&right).unwrap());
        right.insert(6, "!").unwrap();
        assert!(!left.equals(&right).unwrap());
    }

    #[test]
    fn rejects_values_over_capacity_without_partial_insertion() {
        let mut value = SecureTextBuffer::new().unwrap();
        let prefix = "a".repeat(MAX_PASSWORD_BYTES - 2);
        value.insert(0, &prefix).unwrap();
        assert!(matches!(
            value.insert(MAX_PASSWORD_BYTES - 2, "한b"),
            Err(SecureTextEditError::Capacity)
        ));
        assert_eq!(value.insert(MAX_PASSWORD_BYTES - 2, "ab").unwrap(), 2);
    }
}
