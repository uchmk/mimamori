//! Command strings (`screen cpu`, `arrow -1`, `resize left`) to [`Act`].

use tsumugi_layout::Toward;

use crate::screen::Screen;
use crate::svcctl::Action;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Goto {
    To(Screen),
    /// The next built screen, wrapping.
    Next,
    Prev,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Step {
    By(i64),
    Top,
    Bot,
    /// A share of the visible rows: `arrow 50%`, `arrow -100%`.
    Page(f32),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Act {
    Screen(Goto),
    Arrow(Step),
    /// The column to the left (-1) or right (+1).
    Focus(i32),
    /// Move the focused column's divider.
    Resize(Toward),
    /// Every column of this screen back to the same share.
    Equalize,
    /// Open what is selected: a device's screen, a process in procs.
    Enter,
    Filter,
    Sort,
    /// procs as a tree of parents and children, or a flat list.
    Tree,
    /// procs' scope: every process, or only mine (`Some(true)`); `None`
    /// switches to the other.
    Scope(Option<bool>),
    /// Ask, then end the selected process; on svc, stop the service.
    Kill,
    /// Ask, then start, stop or restart the selected service.
    Service(Action),
    Pause,
    /// One step faster (-1) or slower (+1) sampling.
    Interval(i32),
    Help,
    Quit,
    Escape,
    Confirm,
    Backspace,
}

pub fn parse(s: &str) -> Result<Act, String> {
    let mut it = s.split_whitespace();
    let name = it.next().ok_or("empty command")?;
    let arg = it.next();
    if let Some(extra) = it.next() {
        return Err(format!("`{s}`: unexpected `{extra}`"));
    }
    let bad = || format!("`{s}`: bad argument");
    let need = || arg.ok_or_else(|| format!("`{s}`: missing argument"));
    let none = |a: Act| if arg.is_some() { Err(format!("`{s}`: takes no argument")) } else { Ok(a) };
    match name {
        "screen" => Ok(Act::Screen(match need()? {
            "next" => Goto::Next,
            "prev" => Goto::Prev,
            n => Goto::To(Screen::from_name(n).ok_or_else(bad)?),
        })),
        "arrow" => {
            let a = need()?;
            Ok(Act::Arrow(match a {
                "top" => Step::Top,
                "bot" | "bottom" => Step::Bot,
                _ if a.ends_with('%') => Step::Page(a[..a.len() - 1].parse::<f32>().map_err(|_| bad())? / 100.0),
                _ => Step::By(a.parse().map_err(|_| bad())?),
            }))
        }
        "focus" => Ok(Act::Focus(match need()? {
            "left" | "-1" => -1,
            "right" | "+1" | "1" => 1,
            _ => return Err(bad()),
        })),
        "resize" => Ok(Act::Resize(match need()? {
            "left" => Toward::Left,
            "right" => Toward::Right,
            "up" => Toward::Up,
            "down" => Toward::Down,
            _ => return Err(bad()),
        })),
        "interval" => Ok(Act::Interval(match need()? {
            "+" | "slower" => 1,
            "-" | "faster" => -1,
            _ => return Err(bad()),
        })),
        "scope" => Ok(Act::Scope(match arg {
            None => None,
            Some("all") => Some(false),
            Some("mine") => Some(true),
            Some(_) => return Err(bad()),
        })),
        "service" => Ok(Act::Service(match need()? {
            "start" => Action::Start,
            "stop" => Action::Stop,
            "restart" => Action::Restart,
            _ => return Err(bad()),
        })),
        "equalize" => none(Act::Equalize),
        "enter" => none(Act::Enter),
        "filter" => none(Act::Filter),
        "sort" => none(Act::Sort),
        "tree" => none(Act::Tree),
        "kill" => none(Act::Kill),
        "pause" => none(Act::Pause),
        "help" => none(Act::Help),
        "quit" => none(Act::Quit),
        "escape" | "close" => none(Act::Escape),
        "confirm" => none(Act::Confirm),
        "backspace" => none(Act::Backspace),
        _ => Err(format!("unknown command `{name}`")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_commands() {
        assert_eq!(parse("screen cpu"), Ok(Act::Screen(Goto::To(Screen::Cpu))));
        assert_eq!(parse("screen next"), Ok(Act::Screen(Goto::Next)));
        assert_eq!(parse("arrow -1"), Ok(Act::Arrow(Step::By(-1))));
        assert_eq!(parse("arrow 50%"), Ok(Act::Arrow(Step::Page(0.5))));
        assert_eq!(parse("arrow bot"), Ok(Act::Arrow(Step::Bot)));
        assert_eq!(parse("resize up"), Ok(Act::Resize(Toward::Up)));
        assert_eq!(parse("interval +"), Ok(Act::Interval(1)));
        assert_eq!(parse("focus left"), Ok(Act::Focus(-1)));
        assert_eq!(parse("kill"), Ok(Act::Kill));
        assert_eq!(parse("tree"), Ok(Act::Tree));
        assert_eq!(parse("scope"), Ok(Act::Scope(None)));
        assert_eq!(parse("scope mine"), Ok(Act::Scope(Some(true))));
        assert_eq!(parse("scope all"), Ok(Act::Scope(Some(false))));
        assert!(parse("scope others").is_err());
        assert_eq!(parse("service restart"), Ok(Act::Service(Action::Restart)));
        assert_eq!(parse("service stop"), Ok(Act::Service(Action::Stop)));
        assert!(parse("service").is_err());
        assert!(parse("service pause").is_err());
        assert!(parse("screen nowhere").is_err());
        assert!(parse("quit now").is_err());
        assert!(parse("fly").is_err());
    }
}
