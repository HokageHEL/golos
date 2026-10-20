//! macOS shortcut validation; registration remains a separate runtime operation.
use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut};

pub const DEFAULT_SHORTCUT: &str = "CmdOrCtrl+Shift+Space";
#[derive(Debug)]
pub struct ValidatedShortcut {
    pub shortcut: Shortcut,
    pub canonical: String,
    pub display: String,
}
pub fn validate(value: &str) -> Result<ValidatedShortcut, String> {
    let value = value.trim();
    if value.is_empty() || value.len() > 100 {
        return Err("Use a shortcut of up to 100 bytes, such as Command+Shift+Space.".into());
    }
    let shortcut: Shortcut = value.parse().map_err(|_| {
        "Use modifiers followed by one key, such as Command+Shift+Space. Fn and modifier-only shortcuts are unavailable.".to_string()
    })?;
    let mods = shortcut.mods;
    if !mods.intersects(Modifiers::SUPER | Modifiers::CONTROL | Modifiers::ALT) {
        return Err(
            "Include Command, Control or Option so normal typing is not intercepted.".into(),
        );
    }
    let reserved = match shortcut.key {
        Code::Space => [
            Modifiers::SUPER,
            Modifiers::ALT | Modifiers::SUPER,
            Modifiers::CONTROL,
            Modifiers::CONTROL | Modifiers::ALT,
            Modifiers::CONTROL | Modifiers::SUPER,
        ]
        .contains(&mods),
        Code::Digit3 | Code::Digit4 | Code::Digit5 => mods == Modifiers::SUPER | Modifiers::SHIFT,
        Code::KeyQ => [
            Modifiers::SUPER,
            Modifiers::SUPER | Modifiers::CONTROL,
            Modifiers::SUPER | Modifiers::SHIFT,
        ]
        .contains(&mods),
        Code::Escape => [
            Modifiers::SUPER | Modifiers::ALT,
            Modifiers::SUPER | Modifiers::ALT | Modifiers::SHIFT,
        ]
        .contains(&mods),
        Code::Tab | Code::Backquote => mods.contains(Modifiers::SUPER),
        Code::KeyC
        | Code::KeyV
        | Code::KeyX
        | Code::KeyA
        | Code::KeyZ
        | Code::KeyS
        | Code::KeyO
        | Code::KeyN
        | Code::KeyP
        | Code::KeyF
        | Code::KeyH
        | Code::KeyM
        | Code::KeyW => mods == Modifiers::SUPER,
        _ => false,
    };
    if reserved {
        return Err("This shortcut is used for a system or common editing action. Choose a different combination.".into());
    }
    let mut display = String::new();
    for (modifier, label) in [
        (Modifiers::CONTROL, "⌃"),
        (Modifiers::ALT, "⌥"),
        (Modifiers::SHIFT, "⇧"),
        (Modifiers::SUPER, "⌘"),
    ] {
        if mods.contains(modifier) {
            display.push_str(label);
        }
    }
    let key = shortcut.key.to_string();
    display.push_str(
        key.strip_prefix("Key")
            .or_else(|| key.strip_prefix("Digit"))
            .unwrap_or(&key),
    );
    Ok(ValidatedShortcut {
        canonical: shortcut.to_string(),
        shortcut,
        display,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_real_plugin_shortcuts_and_canonicalizes_aliases_without_registering() {
        let default = validate(DEFAULT_SHORTCUT).unwrap();
        assert_eq!(default.display, "⇧⌘Space");
        assert_eq!(
            validate(" command + shift + space ").unwrap().shortcut.id(),
            default.shortcut.id()
        );
        assert_eq!(
            validate(&default.canonical).unwrap().shortcut.id(),
            default.shortcut.id()
        );
        assert_eq!(validate("Control+Option+F5").unwrap().display, "⌃⌥F5");
        assert_eq!(validate("Cmd+Shift+D").unwrap().display, "⇧⌘D");
    }
    #[test]
    fn rejects_typing_modifier_only_invalid_and_reserved_shortcuts() {
        for value in [
            "",
            "A",
            "Space",
            "F5",
            "Shift+A",
            "Fn",
            "Option",
            "Cmd+Shift",
            "Ctrl+A+B",
            "Ctrl+InvalidKey",
            "Cmd+Space",
            "Ctrl+Space",
            "Ctrl+Option+Space",
            "Cmd+Ctrl+Space",
            "Cmd+Shift+3",
            "Cmd+Shift+4",
            "Cmd+Shift+5",
            "Cmd+Q",
            "Cmd+Ctrl+Q",
            "Cmd+Shift+Q",
            "Cmd+Option+Escape",
            "Cmd+Tab",
            "Cmd+Shift+Tab",
            "Cmd+Backquote",
            "Cmd+C",
            "Cmd+V",
            "Cmd+X",
            "Cmd+Z",
            "Cmd+W",
        ] {
            assert!(validate(value).is_err(), "accepted {value}");
        }
        assert!(validate(&"x".repeat(101)).is_err());
    }
}
