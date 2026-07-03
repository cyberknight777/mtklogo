pub use self::explore::run_explore;
pub use self::guess::run_guess;
pub use self::repack::run_repack;
pub use self::unpack::{UnpackRequest, run_unpack};
use colored::{ColoredString, Colorize};
use std::fmt::Display;

mod explore;
mod guess;
mod meta;
mod repack;
mod unpack;

/// formats a command.
pub fn cmd<I: Display + Sized>(input: I) -> ColoredString {
    format!("{}", input).truecolor(255, 153, 51).bold()
}

/// formats a warning message.
pub fn warn<I: Display + Sized>(input: I) -> ColoredString {
    format!("{}", input).truecolor(255, 204, 0).bold()
}

/// formats an error message.
pub fn err<I: Display + Sized>(input: I) -> ColoredString {
    format!("{}", input).truecolor(204, 0, 0)
}

/// emphasizing on a text information.
pub fn emphasize1<I: Display + Sized>(input: I) -> ColoredString {
    format!("{}", input).truecolor(204, 204, 0)
}

/// emphasizing on a text information (variant)
pub fn emphasize2<I: Display + Sized>(input: I) -> ColoredString {
    format!("{}", input).truecolor(102, 153, 0)
}

/// emphasizing on a data.
pub fn data1<I: Display + Sized>(input: I) -> ColoredString {
    format!("{}", input).truecolor(153, 153, 255)
}

/// emphasizing on a data (variant).
pub fn data2<I: Display + Sized>(input: I) -> ColoredString {
    format!("{}", input).truecolor(204, 51, 255)
}

/// emphasizing on a data (variant).
pub fn data3<I: Display + Sized>(input: I) -> ColoredString {
    format!("{}", input).truecolor(51, 204, 255)
}
