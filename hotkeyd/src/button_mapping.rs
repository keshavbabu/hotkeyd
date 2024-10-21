use rdev::Button;

pub fn button_hash(button: Button) -> u32 {
    match button {
        Button::Left => 0,
        Button::Right => 1,
        Button::Middle => 2,
        Button::Unknown(d) => 3 + (d as u32)
    }
}

pub fn unhash_button(button_hash: u32) -> Button {
    match button_hash {
        0 => Button::Left,
        1 => Button::Right,
        2 => Button::Middle,
        3_u32..=u32::MAX => Button::Unknown((button_hash - 3) as u8)
    }
}
