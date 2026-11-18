//! breeze-token's parts, for a program that fetches V3 tokens its own way:
//! the μCore deployer uses them where the `breeze-token` command asks on a
//! terminal.
//!
//! - [`clouds`]: Midea's clouds (which knows an account, signing in, a unit's
//!   token), behind a trait so a test can use a pretend one (the `fake-cloud`
//!   feature).
//! - [`lan`]: the units on this network, and trying a token on one.
//! - [`Fetched`]: a unit with its checked token.
//!
//! The promises are the caller's to keep, as the command keeps them: the
//! password only to a cloud that has the account, every token checked on its
//! unit before it goes anywhere, tokens never printed or logged.
//!
//! [`breeze_cloud`] and [`breeze_proto`] are re-exported so the caller uses
//! the very versions these parts were built with.

pub mod clouds;
pub mod lan;

pub use breeze_cloud;
pub use breeze_proto;

use breeze_cloud::Token;
use breeze_proto::discover::Discovered;

/// A unit, its name on the account, and its checked token.
pub struct Fetched {
    pub unit: Discovered,
    pub name: Option<String>,
    pub token: Token,
}
