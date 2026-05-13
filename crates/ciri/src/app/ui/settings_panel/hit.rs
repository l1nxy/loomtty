//! Hit-id encoding for the settings panel.
//!
//! All settings rows share an encoding scheme so the panel doesn't
//! need a unique constant per control. The scheme:
//!
//! ```text
//!  bits 63..56 : tag (1 = chrome, 2 = sidebar, 3 = field-control)
//!  bits 55..40 : field id (only for tag=3) / chrome op (for tag=1)
//!                / category index (for tag=2)
//!  bits 7..0   : button role (for tag=3 — see [`ButtonRole`])
//! ```
//!
//! Encoding the tag in the high bits keeps panel-owned hit_ids
//! disjoint from the rest of the chrome (top bar, palette, etc.) and
//! from each other.
//!
//! No "what's the hit_id for the close button" constants — both sides
//! call [`encode`]/[`decode`] and let the table do the work.

use super::schema::SettingsField;
use ciri_app::app::SettingsCategory;

const TAG_CHROME: u64 = 1;
const TAG_SIDEBAR: u64 = 2;
const TAG_FIELD: u64 = 3;
const TAG_SHIFT: u64 = 56;
const PAYLOAD_SHIFT: u64 = 8;

/// Chrome op codes — title-bar / footer-level UI elements.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChromeOp {
    Dialog,
    Close,
    OpenToml,
}

impl ChromeOp {
    fn code(self) -> u64 {
        match self {
            Self::Dialog => 0,
            Self::Close => 1,
            Self::OpenToml => 2,
        }
    }
    fn from_code(c: u64) -> Option<Self> {
        Some(match c {
            0 => Self::Dialog,
            1 => Self::Close,
            2 => Self::OpenToml,
            _ => return None,
        })
    }
}

/// Per-control button role (for field rows). One enum here covers
/// stepper +/-, switch tap, and dropdown trigger — the kind of widget
/// is implied by the field's `FieldKind` in the schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonRole {
    /// `−` button on a stepper.
    StepperDec,
    /// `+` button on a stepper.
    StepperInc,
    /// Switch tap target.
    SwitchToggle,
    /// Dropdown trigger.
    DropdownOpen,
}

impl ButtonRole {
    fn code(self) -> u64 {
        match self {
            Self::StepperDec => 1,
            Self::StepperInc => 2,
            Self::SwitchToggle => 3,
            Self::DropdownOpen => 4,
        }
    }
    fn from_code(c: u64) -> Option<Self> {
        Some(match c {
            1 => Self::StepperDec,
            2 => Self::StepperInc,
            3 => Self::SwitchToggle,
            4 => Self::DropdownOpen,
            _ => return None,
        })
    }
}

/// Decoded hit_id payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsHit {
    Chrome(ChromeOp),
    Sidebar(SettingsCategory),
    Field {
        field: SettingsField,
        role: ButtonRole,
    },
}

pub fn encode_chrome(op: ChromeOp) -> u64 {
    (TAG_CHROME << TAG_SHIFT) | (op.code() << PAYLOAD_SHIFT)
}

pub fn encode_sidebar(cat: SettingsCategory) -> u64 {
    let idx = SettingsCategory::ALL
        .iter()
        .position(|c| *c == cat)
        .unwrap_or(0) as u64;
    (TAG_SIDEBAR << TAG_SHIFT) | (idx << PAYLOAD_SHIFT)
}

pub fn encode_field(field: SettingsField, role: ButtonRole) -> u64 {
    (TAG_FIELD << TAG_SHIFT) | ((field.id() as u64) << PAYLOAD_SHIFT) | role.code()
}

pub fn decode(id: u64) -> Option<SettingsHit> {
    let tag = id >> TAG_SHIFT;
    let payload = (id >> PAYLOAD_SHIFT) & 0xFFFF_FFFF_FFFF;
    match tag {
        TAG_CHROME => ChromeOp::from_code(payload).map(SettingsHit::Chrome),
        TAG_SIDEBAR => SettingsCategory::ALL
            .get(payload as usize)
            .copied()
            .map(SettingsHit::Sidebar),
        TAG_FIELD => {
            let role_code = id & 0xFF;
            let field = SettingsField::from_id(payload as u16)?;
            let role = ButtonRole::from_code(role_code)?;
            Some(SettingsHit::Field { field, role })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::ui::settings_panel::schema::FIELDS;

    #[test]
    fn chrome_round_trip() {
        for op in [ChromeOp::Dialog, ChromeOp::Close, ChromeOp::OpenToml] {
            assert_eq!(decode(encode_chrome(op)), Some(SettingsHit::Chrome(op)));
        }
    }

    #[test]
    fn sidebar_round_trip() {
        for cat in SettingsCategory::ALL {
            assert_eq!(
                decode(encode_sidebar(*cat)),
                Some(SettingsHit::Sidebar(*cat))
            );
        }
    }

    #[test]
    fn field_round_trip_for_every_role() {
        for m in FIELDS {
            for role in [
                ButtonRole::StepperDec,
                ButtonRole::StepperInc,
                ButtonRole::SwitchToggle,
                ButtonRole::DropdownOpen,
            ] {
                assert_eq!(
                    decode(encode_field(m.field, role)),
                    Some(SettingsHit::Field {
                        field: m.field,
                        role
                    }),
                    "{:?} / {:?}",
                    m.field,
                    role
                );
            }
        }
    }

    /// Hit-ids from different tags must not collide.
    #[test]
    fn tag_namespaces_are_disjoint() {
        let chrome_ids: Vec<_> = [ChromeOp::Dialog, ChromeOp::Close, ChromeOp::OpenToml]
            .iter()
            .map(|o| encode_chrome(*o))
            .collect();
        let sidebar_ids: Vec<_> = SettingsCategory::ALL
            .iter()
            .map(|c| encode_sidebar(*c))
            .collect();
        let field_ids: Vec<_> = FIELDS
            .iter()
            .map(|m| encode_field(m.field, ButtonRole::StepperDec))
            .collect();
        for c in &chrome_ids {
            assert!(!sidebar_ids.contains(c));
            assert!(!field_ids.contains(c));
        }
        for s in &sidebar_ids {
            assert!(!field_ids.contains(s));
        }
    }
}
