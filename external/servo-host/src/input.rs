//! Pointer and key input of the browser (evdev key codes, device pixels) as Servo input events.

use keyboard_types::{
    Code, Key, KeyState, KeyboardEvent as KeyEvent, Location, Modifiers, NamedKey,
};
use servo::{
    DevicePoint, InputEvent, KeyboardEvent, MouseButton, MouseButtonAction, MouseButtonEvent,
    MouseMoveEvent, WheelDelta, WheelEvent, WheelMode,
};
use ultar_web::engine::{KeyInput, PointerInput};

/// The Servo event for pointer input.
pub fn pointer(input: PointerInput) -> InputEvent {
    let at = |x: f32, y: f32| DevicePoint::new(x, y).into();
    match input {
        PointerInput::Move { x, y } => InputEvent::MouseMove(MouseMoveEvent::new(at(x, y))),
        PointerInput::Down { x, y, button } => InputEvent::MouseButton(MouseButtonEvent::new(
            MouseButtonAction::Down,
            mouse_button(button),
            at(x, y),
        )),
        PointerInput::Up { x, y, button } => InputEvent::MouseButton(MouseButtonEvent::new(
            MouseButtonAction::Up,
            mouse_button(button),
            at(x, y),
        )),
        // the browser counts "down" as positive, Servo counts "towards the top" as positive
        PointerInput::Scroll { x, y, dx, dy } => InputEvent::Wheel(WheelEvent::new(
            WheelDelta {
                x: -f64::from(dx),
                y: -f64::from(dy),
                z: 0.0,
                mode: WheelMode::DeltaPixel,
            },
            at(x, y),
        )),
    }
}

fn mouse_button(button: u8) -> MouseButton {
    match button {
        1 => MouseButton::Primary,
        2 => MouseButton::Auxiliary,
        3 => MouseButton::Secondary,
        8 => MouseButton::Back,
        9 => MouseButton::Forward,
        b => MouseButton::Other(u16::from(b)),
    }
}

/// The Servo event for a key.
pub fn key(input: KeyInput) -> InputEvent {
    let code = code_of(input.code);
    let key = key_of(input);
    let mut event = if input.pressed {
        KeyEvent::key_down(key, code)
    } else {
        KeyEvent::key_up(key, code)
    };
    event.state = if input.pressed {
        KeyState::Down
    } else {
        KeyState::Up
    };
    event.location = location_of(input.code);
    event.modifiers = modifiers_of(input.modifiers);
    InputEvent::Keyboard(KeyboardEvent::new(event))
}

fn modifiers_of(bits: u8) -> Modifiers {
    let mut m = Modifiers::empty();
    for (bit, flag) in [
        (1, Modifiers::SHIFT),
        (2, Modifiers::CONTROL),
        (4, Modifiers::ALT),
        (8, Modifiers::META),
    ] {
        if bits & bit != 0 {
            m |= flag;
        }
    }
    m
}

/// The key value: the character the key produced, else the named key of its code.
fn key_of(input: KeyInput) -> Key {
    let printable = char::from_u32(input.character).filter(|c| !c.is_control());
    match (printable, named_of(input.code)) {
        // Enter, Tab, Backspace … keep their names even when a character came with them
        (_, Some(named)) => Key::Named(named),
        (Some(c), None) => Key::Character(c.to_string()),
        (None, None) => Key::Named(NamedKey::Unidentified),
    }
}

fn named_of(code: u16) -> Option<NamedKey> {
    Some(match code {
        1 => NamedKey::Escape,
        14 => NamedKey::Backspace,
        15 => NamedKey::Tab,
        28 | 96 => NamedKey::Enter,
        29 | 97 => NamedKey::Control,
        42 | 54 => NamedKey::Shift,
        56 | 100 => NamedKey::Alt,
        58 => NamedKey::CapsLock,
        59..=68 => f_key(code - 58)?,
        87 => NamedKey::F11,
        88 => NamedKey::F12,
        102 => NamedKey::Home,
        103 => NamedKey::ArrowUp,
        104 => NamedKey::PageUp,
        105 => NamedKey::ArrowLeft,
        106 => NamedKey::ArrowRight,
        107 => NamedKey::End,
        108 => NamedKey::ArrowDown,
        109 => NamedKey::PageDown,
        110 => NamedKey::Insert,
        111 => NamedKey::Delete,
        125 | 126 => NamedKey::Meta,
        127 => NamedKey::ContextMenu,
        _ => return None,
    })
}

fn f_key(n: u16) -> Option<NamedKey> {
    use NamedKey::*;
    [F1, F2, F3, F4, F5, F6, F7, F8, F9, F10]
        .get(usize::from(n.checked_sub(1)?))
        .cloned()
}

fn location_of(code: u16) -> Location {
    match code {
        29 | 42 | 56 | 125 => Location::Left,
        54 | 97 | 100 | 126 => Location::Right,
        71..=83 | 96 | 98 | 55 => Location::Numpad,
        _ => Location::Standard,
    }
}

/// The physical key of an evdev code (the US layout positions the DOM `code` names).
fn code_of(code: u16) -> Code {
    use Code::*;
    const LETTERS_Q: [Code; 10] = [KeyQ, KeyW, KeyE, KeyR, KeyT, KeyY, KeyU, KeyI, KeyO, KeyP];
    const LETTERS_A: [Code; 9] = [KeyA, KeyS, KeyD, KeyF, KeyG, KeyH, KeyJ, KeyK, KeyL];
    const LETTERS_Z: [Code; 7] = [KeyZ, KeyX, KeyC, KeyV, KeyB, KeyN, KeyM];
    const DIGITS: [Code; 10] = [
        Digit1, Digit2, Digit3, Digit4, Digit5, Digit6, Digit7, Digit8, Digit9, Digit0,
    ];
    let pick = |table: &[Code], first: u16| table[usize::from(code - first)];
    match code {
        2..=11 => pick(&DIGITS, 2),
        16..=25 => pick(&LETTERS_Q, 16),
        30..=38 => pick(&LETTERS_A, 30),
        44..=50 => pick(&LETTERS_Z, 44),
        1 => Escape,
        12 => Minus,
        13 => Equal,
        14 => Backspace,
        15 => Tab,
        26 => BracketLeft,
        27 => BracketRight,
        28 => Enter,
        29 => ControlLeft,
        39 => Semicolon,
        40 => Quote,
        41 => Backquote,
        42 => ShiftLeft,
        43 => Backslash,
        51 => Comma,
        52 => Period,
        53 => Slash,
        54 => ShiftRight,
        56 => AltLeft,
        57 => Space,
        58 => CapsLock,
        97 => ControlRight,
        100 => AltRight,
        102 => Home,
        103 => ArrowUp,
        104 => PageUp,
        105 => ArrowLeft,
        106 => ArrowRight,
        107 => End,
        108 => ArrowDown,
        109 => PageDown,
        110 => Insert,
        111 => Delete,
        125 => MetaLeft,
        126 => MetaRight,
        _ => Unidentified,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_carry_their_character_or_their_name() {
        let arabic = KeyInput {
            code: 30,
            character: u32::from('ش'),
            modifiers: 0,
            pressed: true,
        };
        assert_eq!(key_of(arabic), Key::Character("ش".into()));
        assert_eq!(code_of(30), Code::KeyA);
        let enter = KeyInput {
            code: 28,
            character: u32::from('\r'),
            modifiers: 0,
            pressed: true,
        };
        assert_eq!(key_of(enter), Key::Named(NamedKey::Enter));
        assert_eq!(named_of(68), Some(NamedKey::F10));
        assert_eq!(code_of(11), Code::Digit0);
        assert!(modifiers_of(3).contains(Modifiers::SHIFT | Modifiers::CONTROL));
    }
}
