//! Windows and dialogs opened from the menu / toolbar.

#[cfg(windows)]
mod assoc;
mod bookmarks;
mod colors;
mod export;
mod help;
pub(in crate::app) mod logsets;
mod lookup;
mod rules;
mod tabexport;
mod update;
