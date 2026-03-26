pub mod names;
pub mod restore;
pub mod save;
pub mod state;
pub mod template;

pub(crate) fn is_plain_name(name: &str) -> bool {
    !name.is_empty() && !name.contains('/') && !name.contains('\\') && !name.contains("..")
}
