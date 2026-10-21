//! Generic, manually approved launcher Mod packages. No game/account business code.
pub mod package;
pub mod process;
pub type Result<T> = std::result::Result<T, String>;
