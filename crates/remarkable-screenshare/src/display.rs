//! Frame Display using minifb
//!
//! Provides a window to display received framebuffer data.
//! Supports:
//! - Real-time display of screen share
//! - Input capture (mouse/keyboard) for injection
//! - Window scaling

use std::time::Duration;

use minifb::{Key, MouseButton, MouseMode, Scale, Window, WindowOptions};
use thiserror::Error;
use tracing::info;

use crate::rfb::{FB_HEIGHT, FB_WIDTH};

/// Display errors
#[derive(Error, Debug)]
pub enum DisplayError {
    #[error("Window creation failed: {0}")]
    WindowCreate(String),

    #[error("Buffer update failed: {0}")]
    BufferUpdate(String),

    #[error("Window closed")]
    WindowClosed,
}

/// Input event from display window
#[derive(Debug, Clone)]
pub enum InputEvent {
    /// Key press/release
    Key {
        key: Key,
        down: bool,
    },
    /// Mouse button press/release
    MouseButton {
        button: MouseButton,
        down: bool,
        x: u16,
        y: u16,
    },
    /// Mouse move
    MouseMove {
        x: u16,
        y: u16,
    },
}

/// Display window for framebuffer
pub struct Display {
    window: Window,
    buffer: Vec<u32>,
    width: usize,
    height: usize,
    last_mouse_pos: Option<(f32, f32)>,
    last_keys: Vec<Key>,
}

impl Display {
    /// Create new display window
    pub fn new(title: &str) -> Result<Self, DisplayError> {
        Self::with_dimensions(title, FB_WIDTH as usize, FB_HEIGHT as usize)
    }

    /// Create display with custom dimensions
    pub fn with_dimensions(title: &str, width: usize, height: usize) -> Result<Self, DisplayError> {
        let options = WindowOptions {
            resize: true,
            scale: Scale::X1,
            scale_mode: minifb::ScaleMode::AspectRatioStretch,
            ..Default::default()
        };

        let mut window = Window::new(title, width, height, options)
            .map_err(|e| DisplayError::WindowCreate(e.to_string()))?;

        // Set frame rate limit
        window.set_target_fps(60);

        let buffer = vec![0u32; width * height];

        info!(width, height, title, "Created display window");

        Ok(Self {
            window,
            buffer,
            width,
            height,
            last_mouse_pos: None,
            last_keys: Vec::new(),
        })
    }

    /// Check if window is open
    pub fn is_open(&self) -> bool {
        self.window.is_open()
    }

    /// Check for escape key
    pub fn should_close(&self) -> bool {
        !self.window.is_open() || self.window.is_key_down(Key::Escape)
    }

    /// Update display with grayscale framebuffer (8-bit per pixel)
    pub fn update_grayscale(&mut self, data: &[u8]) -> Result<(), DisplayError> {
        if data.len() != self.width * self.height {
            return Err(DisplayError::BufferUpdate(format!(
                "Buffer size mismatch: got {}, expected {}",
                data.len(),
                self.width * self.height
            )));
        }

        // Convert grayscale to RGB32
        for (i, &gray) in data.iter().enumerate() {
            // reMarkable inverts colors (0 = white, 255 = black)
            let g = 255 - gray;
            self.buffer[i] = ((g as u32) << 16) | ((g as u32) << 8) | (g as u32);
        }

        self.window
            .update_with_buffer(&self.buffer, self.width, self.height)
            .map_err(|e| DisplayError::BufferUpdate(e.to_string()))?;

        Ok(())
    }

    /// Update display with RGB32 buffer
    pub fn update_rgb32(&mut self, data: &[u32]) -> Result<(), DisplayError> {
        if data.len() != self.width * self.height {
            return Err(DisplayError::BufferUpdate(format!(
                "Buffer size mismatch: got {}, expected {}",
                data.len(),
                self.width * self.height
            )));
        }

        self.buffer.copy_from_slice(data);

        self.window
            .update_with_buffer(&self.buffer, self.width, self.height)
            .map_err(|e| DisplayError::BufferUpdate(e.to_string()))?;

        Ok(())
    }

    /// Get pending input events
    pub fn poll_input(&mut self) -> Vec<InputEvent> {
        let mut events = Vec::new();

        // Check for key events
        let current_keys: Vec<Key> = self
            .window
            .get_keys();

        // Detect key presses
        for key in &current_keys {
            if !self.last_keys.contains(key) {
                events.push(InputEvent::Key {
                    key: *key,
                    down: true,
                });
            }
        }

        // Detect key releases
        for key in &self.last_keys {
            if !current_keys.contains(key) {
                events.push(InputEvent::Key {
                    key: *key,
                    down: false,
                });
            }
        }

        self.last_keys = current_keys;

        // Check mouse position
        if let Some((x, y)) = self.window.get_mouse_pos(MouseMode::Clamp) {
            let x = x.round() as u16;
            let y = y.round() as u16;

            let pos_changed = match self.last_mouse_pos {
                Some((lx, ly)) => (lx - x as f32).abs() > 0.5 || (ly - y as f32).abs() > 0.5,
                None => true,
            };

            if pos_changed {
                events.push(InputEvent::MouseMove { x, y });
                self.last_mouse_pos = Some((x as f32, y as f32));
            }

            // Check mouse buttons
            for button in [MouseButton::Left, MouseButton::Right, MouseButton::Middle] {
                if self.window.get_mouse_down(button) {
                    events.push(InputEvent::MouseButton {
                        button,
                        down: true,
                        x,
                        y,
                    });
                }
            }
        }

        events
    }

    /// Wait for next frame (respects target FPS)
    pub fn wait(&self) {
        std::thread::sleep(Duration::from_millis(16)); // ~60 FPS
    }

    /// Get window dimensions
    pub fn dimensions(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    /// Get current framebuffer as RGB32
    pub fn framebuffer(&self) -> &[u32] {
        &self.buffer
    }

    /// Get current framebuffer as grayscale bytes
    pub fn framebuffer_grayscale(&self) -> Vec<u8> {
        self.buffer
            .iter()
            .map(|rgb| ((rgb >> 16) & 0xFF) as u8)
            .collect()
    }
}

/// Convert minifb key to RFB keysym
pub fn key_to_keysym(key: Key) -> Option<u32> {
    // X11 keysyms
    Some(match key {
        Key::A => 0x61,
        Key::B => 0x62,
        Key::C => 0x63,
        Key::D => 0x64,
        Key::E => 0x65,
        Key::F => 0x66,
        Key::G => 0x67,
        Key::H => 0x68,
        Key::I => 0x69,
        Key::J => 0x6a,
        Key::K => 0x6b,
        Key::L => 0x6c,
        Key::M => 0x6d,
        Key::N => 0x6e,
        Key::O => 0x6f,
        Key::P => 0x70,
        Key::Q => 0x71,
        Key::R => 0x72,
        Key::S => 0x73,
        Key::T => 0x74,
        Key::U => 0x75,
        Key::V => 0x76,
        Key::W => 0x77,
        Key::X => 0x78,
        Key::Y => 0x79,
        Key::Z => 0x7a,
        Key::Key0 => 0x30,
        Key::Key1 => 0x31,
        Key::Key2 => 0x32,
        Key::Key3 => 0x33,
        Key::Key4 => 0x34,
        Key::Key5 => 0x35,
        Key::Key6 => 0x36,
        Key::Key7 => 0x37,
        Key::Key8 => 0x38,
        Key::Key9 => 0x39,
        Key::Space => 0x20,
        Key::Enter => 0xff0d,
        Key::Backspace => 0xff08,
        Key::Tab => 0xff09,
        Key::Escape => 0xff1b,
        Key::Up => 0xff52,
        Key::Down => 0xff54,
        Key::Left => 0xff51,
        Key::Right => 0xff53,
        Key::Home => 0xff50,
        Key::End => 0xff57,
        Key::PageUp => 0xff55,
        Key::PageDown => 0xff56,
        Key::Insert => 0xff63,
        Key::Delete => 0xffff,
        Key::LeftShift | Key::RightShift => 0xffe1,
        Key::LeftCtrl | Key::RightCtrl => 0xffe3,
        Key::LeftAlt | Key::RightAlt => 0xffe9,
        _ => return None,
    })
}

/// Convert minifb mouse button to RFB button mask
pub fn mouse_button_mask(button: MouseButton, down: bool) -> u8 {
    let bit = match button {
        MouseButton::Left => 0x01,
        MouseButton::Middle => 0x02,
        MouseButton::Right => 0x04,
    };
    if down {
        bit
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_to_keysym() {
        assert_eq!(key_to_keysym(Key::A), Some(0x61));
        assert_eq!(key_to_keysym(Key::Enter), Some(0xff0d));
        assert_eq!(key_to_keysym(Key::Space), Some(0x20));
    }

    #[test]
    fn test_mouse_button_mask() {
        assert_eq!(mouse_button_mask(MouseButton::Left, true), 0x01);
        assert_eq!(mouse_button_mask(MouseButton::Right, true), 0x04);
        assert_eq!(mouse_button_mask(MouseButton::Left, false), 0x00);
    }
}
